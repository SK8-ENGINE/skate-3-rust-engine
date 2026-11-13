//! The camcorder LCD panel look, ported from SkateLauncher.cs (LcdPanel, LcdCanvas and their helpers; doc 41, 44a 6)
//! function by function with its coordinates, colors, fonts and timings: a 1240 x 760 unit space for the panel, and
//! the LCD laid out in a 340-wide space over the dusk skate-spot scene.
use std::sync::Arc;

use eframe::egui::{self, epaint, pos2, vec2, Color32, FontFamily, FontId, Pos2, Rect, Shape, Stroke};

pub const DEV: egui::Vec2 = vec2(1240.0, 760.0);
const PANEL_R: Rect = Rect { min: pos2(20.0, 20.0), max: pos2(1192.0, 740.0) };
const SCREEN_R: Rect = Rect { min: pos2(172.0, 52.0), max: pos2(1162.0, 609.0) };

const PLASTIC: Color32 = Color32::from_rgb(54, 56, 62);
const PLASTIC_HI: Color32 = Color32::from_rgb(100, 103, 112);
const PLASTIC_DARK: Color32 = Color32::from_rgb(24, 25, 29);
const PRINT: Color32 = Color32::from_rgb(200, 202, 208);
const REC_RED: Color32 = Color32::from_rgb(230, 40, 32);
const LCD_INK: Color32 = Color32::from_rgb(244, 246, 250);
const LCD_DIM: Color32 = Color32::from_rgb(168, 178, 196);
const LCD_AMBER_HI: Color32 = Color32::from_rgb(255, 198, 88);
const LCD_AMBER_LO: Color32 = Color32::from_rgb(238, 124, 12);
const LCD_AMBER: Color32 = Color32::from_rgb(255, 172, 44);
const LCD_GREEN: Color32 = Color32::from_rgb(124, 220, 96);
const LCD_RED: Color32 = Color32::from_rgb(255, 98, 86);

/// .NET's System.Random (the subtractive generator), so seeded layouts (the skyline, the wear) match the C# window.
pub struct NetRandom {
    seed: [i32; 56],
    inext: usize,
    inextp: usize,
}

impl NetRandom {
    pub fn new(seed: i32) -> NetRandom {
        let mut sa = [0i32; 56];
        let subtraction = if seed == i32::MIN { i32::MAX } else { seed.abs() };
        let mut mj = 161803398i32.wrapping_sub(subtraction);
        sa[55] = mj;
        let mut mk = 1i32;
        for i in 1..55 {
            let ii = (21 * i) % 55;
            sa[ii] = mk;
            mk = mj.wrapping_sub(mk);
            if mk < 0 { mk = mk.wrapping_add(i32::MAX); }
            mj = sa[ii];
        }
        for _ in 1..5 {
            for i in 1..56 {
                sa[i] = sa[i].wrapping_sub(sa[1 + (i + 30) % 55]);
                if sa[i] < 0 { sa[i] = sa[i].wrapping_add(i32::MAX); }
            }
        }
        NetRandom { seed: sa, inext: 0, inextp: 21 }
    }
    fn internal_sample(&mut self) -> i32 {
        self.inext = if self.inext + 1 >= 56 { 1 } else { self.inext + 1 };
        self.inextp = if self.inextp + 1 >= 56 { 1 } else { self.inextp + 1 };
        let mut r = self.seed[self.inext].wrapping_sub(self.seed[self.inextp]);
        if r == i32::MAX { r -= 1; }
        if r < 0 { r = r.wrapping_add(i32::MAX); }
        self.seed[self.inext] = r;
        r
    }
    pub fn next_double(&mut self) -> f64 {
        self.internal_sample() as f64 * (1.0 / i32::MAX as f64)
    }
    /// `Next()`: the raw sample.
    pub fn next_raw(&mut self) -> i32 {
        self.internal_sample()
    }
    /// `Next(max)`.
    pub fn next(&mut self, max: i32) -> i32 {
        (self.next_double() * max as f64) as i32
    }
}

/// Fonts: the C# window's Segoe UI / Tahoma / Consolas from the system (Windows); elsewhere egui's own.
pub fn install_fonts(ctx: &egui::Context) {
    let mut defs = egui::FontDefinitions::default();
    let dir = std::env::var_os("WINDIR").map_or_else(|| std::path::PathBuf::from(r"C:\Windows"), std::path::PathBuf::from).join("Fonts");
    let unix: [&str; 4] = ["/usr/share/fonts/truetype/dejavu", "/usr/share/fonts/TTF", "/System/Library/Fonts", "/Library/Fonts"];
    let families = [
        ("seg", vec!["segoeui.ttf", "DejaVuSans.ttf", "Helvetica.ttc"]),
        ("segsb", vec!["seguisb.ttf", "segoeuib.ttf", "DejaVuSans-Bold.ttf", "Helvetica.ttc"]),
        ("segl", vec!["segoeuil.ttf", "segoeui.ttf", "DejaVuSans-ExtraLight.ttf", "Helvetica.ttc"]),
        ("tahb", vec!["tahomabd.ttf", "DejaVuSans-Bold.ttf", "Helvetica.ttc"]),
        ("tah", vec!["tahoma.ttf", "DejaVuSans.ttf", "Helvetica.ttc"]),
    ];
    for (name, files) in families {
        let found = files.iter().find_map(|f| {
            std::iter::once(dir.join(f)).chain(unix.iter().map(|d| std::path::Path::new(d).join(f))).find_map(|p| std::fs::read(p).ok())
        });
        let mut list = Vec::new();
        if let Some(bytes) = found {
            defs.font_data.insert(name.to_owned(), Arc::new(egui::FontData::from_owned(bytes)));
            list.push(name.to_owned());
        }
        list.extend(defs.families.get(&FontFamily::Proportional).cloned().unwrap_or_default());
        defs.families.insert(FontFamily::Name(name.into()), list);
    }
    ctx.set_fonts(defs);
}

/// A GDI font in pixels: family and em size; `height()` is GDI's line height (Font.Height).
#[derive(Clone, Copy)]
struct Font {
    family: &'static str,
    size: f32,
}

impl Font {
    const fn new(family: &'static str, size: f32) -> Font {
        Font { family, size }
    }
    fn height(&self) -> f32 {
        (self.size * 1.33).ceil()
    }
}

const U_TINY: Font = Font::new("tah", 13.0);
const U_KEY: Font = Font::new("tahb", 19.0);
const U_MODEL: Font = Font::new("tahb", 18.0);

#[derive(Clone, Copy, PartialEq)]
enum Align {
    Near,
    Center,
    Far,
}

/// The bezel and LCD keys: what a click on them means (the C# key names).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Key {
    Menu,
    Up,
    Down,
    A,
    Play,
    Power,
    Display,
    Left,
    Right,
}

/// What the LCD shows (from ui.rs).
pub struct View<'a> {
    pub main: bool,
    pub title: &'a str,
    pub items: Vec<(&'a str, bool)>,
    pub sel: usize,
    pub body: Option<String>,
    pub mono: Option<String>,
    pub info: &'a str,
    pub hint: &'a str,
    pub records: &'a [String],
    pub version: &'a str,
    pub version_status: &'a str,
    pub last: &'a str,
    pub send: &'a str,
    pub job: Option<JobView<'a>>,
    pub recording: bool,
    pub tape: f32,
    pub splash: Option<&'a str>,
    /// The start check: seconds since it began, and Some(armed) while B can skip its self-update fetch.
    pub splash_secs: f32,
    pub splash_skip: Option<bool>,
    pub hover: Option<Key>,
    pub cancel_armed: bool,
    pub close_in: Option<u32>,
}

pub struct JobView<'a> {
    pub title: &'a str,
    pub building: Option<&'a str>,
    pub commit: &'a str,
    pub progress: Option<(u32, u32)>,
    pub compiled: u32,
    pub warnings: u32,
    pub now: &'a str,
    pub waiting: Option<&'a str>,
    pub stage: &'a str,
    pub ended: Option<bool>,
    pub error: &'a [String],
    pub lines: Vec<&'a str>,
    pub elapsed: f32,
    pub wait_who: Option<&'a str>,
    pub wait_secs: f32,
    /// A fetch the backend has waited on for 5 s or more: what, and for how long.
    pub fetch_wait: Option<(&'a str, f32)>,
    pub built_secs: Option<f32>,
}

#[derive(Default)]
pub struct Hits {
    pub item: Option<usize>,
    pub key: Option<Key>,
    pub hover: Option<Key>,
    pub drag: bool,
    /// The list row under the pointer (hover selects it, as in the C# window).
    pub hover_item: Option<usize>,
    pub right_click: bool,
    /// A footer prompt (A / B / START/STOP badge with its label) under the pointer, and the one clicked.
    pub prompt_hover: Option<String>,
    pub prompt: Option<String>,
    /// A left click on neither a row nor a key (closes the direct-mode countdown).
    pub empty_click: bool,
}

fn rgba(a: u8, c: Color32) -> Color32 {
    let [r, g, b, _] = c.to_srgba_unmultiplied();
    Color32::from_rgba_unmultiplied(r, g, b, a)
}
fn argb(a: u8, r: u8, g: u8, b: u8) -> Color32 {
    Color32::from_rgba_unmultiplied(r, g, b, a)
}
fn mix(a: Color32, b: Color32, k: f32) -> Color32 {
    // Color32 holds premultiplied channels: mix the unmultiplied ones, or the alpha is applied twice.
    let m = |x: u8, y: u8| (x as f32 + (y as f32 - x as f32) * k) as u8;
    let (a, b) = (a.to_srgba_unmultiplied(), b.to_srgba_unmultiplied());
    Color32::from_rgba_unmultiplied(m(a[0], b[0]), m(a[1], b[1]), m(a[2], b[2]), a[3])
}
fn lerp(a: Color32, b: Color32, t: f32) -> Color32 {
    let t = t.clamp(0.0, 1.0);
    let m = |x: u8, y: u8| (x as f32 + (y as f32 - x as f32) * t).round() as u8;
    let (a, b) = (a.to_srgba_unmultiplied(), b.to_srgba_unmultiplied());
    Color32::from_rgba_unmultiplied(m(a[0], b[0]), m(a[1], b[1]), m(a[2], b[2]), m(a[3], b[3]))
}
fn grow(r: Rect, d: f32) -> Rect {
    r.expand(d)
}

/// Maps a drawing space into the window: an affine scale + offset, plus the panel swing squashing x towards the hinge.
#[derive(Clone, Copy)]
struct Xf {
    ox: f32,
    oy: f32,
    sx: f32,
    sy: f32,
    swing: f32,
    hinge: f32,
}

impl Xf {
    fn p(&self, x: f32, y: f32) -> Pos2 {
        let ux = self.ox + x * self.sx;
        let uy = self.oy + y * self.sy;
        pos2(self.hinge + (ux - self.hinge) * self.swing, uy)
    }
    fn pt(&self, p: Pos2) -> Pos2 {
        self.p(p.x, p.y)
    }
    fn r(&self, r: Rect) -> Rect {
        Rect::from_two_pos(self.pt(r.min), self.pt(r.max))
    }
    fn s(&self, v: f32) -> f32 {
        v * self.sy
    }
}

/// `Round`: a rounded rectangle's outline points.
fn round_pts(r: Rect, rad: f32) -> Vec<Pos2> {
    let d = (rad * 2.0).min(r.width()).min(r.height());
    let rr = d / 2.0;
    let mut pts = Vec::with_capacity(40);
    let corners = [(r.left() + rr, r.top() + rr, 180.0f32), (r.right() - rr, r.top() + rr, 270.0), (r.right() - rr, r.bottom() - rr, 0.0), (r.left() + rr, r.bottom() - rr, 90.0)];
    for (cx, cy, start) in corners {
        for k in 0..=8 {
            let a = (start + 90.0 * k as f32 / 8.0).to_radians();
            pts.push(pos2(cx + rr * a.cos(), cy + rr * a.sin()));
        }
    }
    pts
}

struct Ctx<'a> {
    painter: &'a egui::Painter,
    xf: Xf,
}

impl Ctx<'_> {
    /// A convex polygon filled with a vertical gradient between `top` at `y0` and `bottom` at `y1`.
    fn fill_vgrad(&self, pts: &[Pos2], y0: f32, y1: f32, top: Color32, bottom: Color32) {
        if pts.len() < 3 { return; }
        let mut mesh = egui::Mesh::default();
        let c = pts.iter().fold(vec2(0.0, 0.0), |a, p| a + p.to_vec2()) / pts.len() as f32;
        let col = |y: f32| lerp(top, bottom, if (y1 - y0).abs() < 1e-3 { 0.0 } else { (y - y0) / (y1 - y0) });
        mesh.colored_vertex(self.xf.p(c.x, c.y), col(c.y));
        for p in pts {
            mesh.colored_vertex(self.xf.pt(*p), col(p.y));
        }
        let n = pts.len() as u32;
        for i in 0..n {
            mesh.add_triangle(0, 1 + i, 1 + (i + 1) % n);
        }
        self.painter.add(Shape::mesh(mesh));
    }
    fn fill_poly(&self, pts: &[Pos2], c: Color32) {
        self.fill_vgrad(pts, 0.0, 1.0, c, c);
    }
    fn stroke_poly(&self, pts: &[Pos2], w: f32, c: Color32) {
        let p: Vec<Pos2> = pts.iter().map(|p| self.xf.pt(*p)).collect();
        self.painter.add(Shape::closed_line(p, Stroke::new(self.xf.s(w), c)));
    }
    fn line(&self, a: Pos2, b: Pos2, w: f32, c: Color32) {
        self.painter.line_segment([self.xf.pt(a), self.xf.pt(b)], Stroke::new(self.xf.s(w), c));
    }
    fn lines(&self, pts: &[Pos2], w: f32, c: Color32) {
        self.painter.add(Shape::line(pts.iter().map(|p| self.xf.pt(*p)).collect(), Stroke::new(self.xf.s(w), c)));
    }
    fn ellipse_pts(r: Rect) -> Vec<Pos2> {
        (0..32).map(|i| { let a = i as f32 / 32.0 * std::f32::consts::TAU; pos2(r.center().x + r.width() / 2.0 * a.cos(), r.center().y + r.height() / 2.0 * a.sin()) }).collect()
    }
    fn fill_ellipse(&self, r: Rect, c: Color32) {
        self.fill_poly(&Self::ellipse_pts(r), c);
    }
    fn fill_ellipse_vgrad(&self, r: Rect, top: Color32, bottom: Color32) {
        self.fill_vgrad(&Self::ellipse_pts(r), r.top(), r.bottom(), top, bottom);
    }
    fn fill_rect(&self, r: Rect, c: Color32) {
        self.painter.rect_filled(self.xf.r(r), 0.0, c);
    }
    /// `Card`: a rounded rect with a vertical gradient and an edge.
    fn card(&self, r: Rect, rad: f32, top: Color32, bottom: Color32, edge: Color32, edge_w: f32) {
        let pts = round_pts(r, rad);
        self.fill_vgrad(&pts, r.top(), r.bottom(), top, bottom);
        if edge_w > 0.0 { self.stroke_poly(&pts, edge_w, edge); }
    }
    /// `Write`: text in a rect, aligned, cut with an ellipsis; `wrap` keeps whole lines only.
    fn write(&self, s: &str, f: Font, c: Color32, r: Rect, h: Align, v: Align, wrap: bool) {
        if s.is_empty() || r.width() <= 0.0 { return; }
        let size = self.xf.s(f.size);
        // GDI pads a string by 1/6 em on each side (StringFormat.GenericDefault).
        let pad = self.xf.s(f.size / 6.0);
        let width = (self.xf.s(r.width()) - 2.0 * pad).max(1.0);
        let mut job = epaint::text::LayoutJob::single_section(s.to_owned(), egui::TextFormat::simple(FontId::new(size, FontFamily::Name(f.family.into())), c));
        let line_h = self.xf.s(f.height());
        let max_rows = if wrap { ((self.xf.s(r.height()) / line_h).floor() as usize).max(1) } else { 1 };
        job.wrap = epaint::text::TextWrapping { max_width: width, max_rows, break_anywhere: false, overflow_character: Some('…') };
        job.halign = match h { Align::Near => egui::Align::LEFT, Align::Center => egui::Align::Center, Align::Far => egui::Align::RIGHT };
        let galley = self.painter.layout_job(job);
        let rr = self.xf.r(r);
        let x = match h { Align::Near => rr.left() + pad, Align::Center => rr.center().x, Align::Far => rr.right() - pad };
        let y = match v { Align::Near => rr.top() + (line_h - size * 1.2).max(0.0) * 0.35, Align::Center => rr.center().y - galley.size().y / 2.0, Align::Far => rr.bottom() - galley.size().y };
        self.painter.with_clip_rect(rr.expand(self.xf.s(1.0))).galley(pos2(x, y), galley, c);
    }
    /// `MeasureString` width.
    fn measure(&self, s: &str, f: Font) -> f32 {
        let galley = self.painter.layout_no_wrap(s.to_owned(), FontId::new(self.xf.s(f.size), FontFamily::Name(f.family.into())), Color32::WHITE);
        // GDI MeasureString: the advance plus 1/6 em of padding on each side.
        galley.size().x / self.xf.sy + f.size / 3.0
    }
    /// `MeasureString` height with wrapping at `w`.
    fn measure_h(&self, s: &str, f: Font, w: f32) -> f32 {
        let galley = self.painter.layout(s.to_owned(), FontId::new(self.xf.s(f.size), FontFamily::Name(f.family.into())), Color32::WHITE, (self.xf.s(w) - self.xf.s(f.size / 3.0)).max(1.0));
        galley.rows.len() as f32 * f.height()
    }
    fn shadow_text(&self, s: &str, f: Font, c: Color32, x: f32, y: f32) {
        let r = Rect::from_min_size(pos2(x + 0.6, y + 0.7), vec2(400.0, f.height()));
        self.write(s, f, argb(170, 0, 0, 0), r, Align::Near, Align::Near, false);
        self.write(s, f, c, Rect::from_min_size(pos2(x, y), vec2(400.0, f.height())), Align::Near, Align::Near, false);
    }
    /// `Glass`: a translucent panel with a fine light edge.
    fn glass(&self, r: Rect, rad: f32) {
        let pts = round_pts(r, rad);
        self.fill_vgrad(&pts, r.top() - 1.0, r.bottom() + 1.0, argb(196, 44, 54, 80), argb(222, 10, 13, 22));
        self.stroke_poly(&pts, 0.6, argb(76, 255, 255, 255));
        self.line(pos2(r.left() + rad, r.top() + 1.0), pos2(r.right() - rad, r.top() + 1.0), 0.6, argb(46, 255, 255, 255));
    }
    /// `Gloss`: a glossy bar with a sheen over its top half.
    fn gloss(&self, r: Rect, rad: f32, top: Color32, bottom: Color32) {
        let pts = round_pts(r, rad);
        self.fill_vgrad(&pts, r.top() - 0.5, r.bottom() + 0.5, top, bottom);
        let sheen = Rect::from_min_size(pos2(r.left() + 0.8, r.top() + 0.5), vec2(r.width() - 1.6, r.height() * 0.48));
        if sheen.width() > 1.0 && sheen.height() > 0.5 {
            self.fill_vgrad(&round_pts(sheen, rad * 0.8), sheen.top() - 0.5, sheen.bottom() + 0.5, argb(120, 255, 255, 255), argb(18, 255, 255, 255));
        }
    }
}

/// The LCD fonts, sized from the layout height (LcdCanvas).
struct Osd {
    big: Font,
    row: Font,
    small: Font,
    tiny: Font,
    logo: Font,
}

impl Osd {
    fn new(h: f32) -> Osd {
        Osd { big: Font::new("segsb", h / 13.5), row: Font::new("segsb", h / 17.5), small: Font::new("seg", h / 21.0), tiny: Font::new("segsb", h / 29.0), logo: Font::new("segl", h / 5.2) }
    }
}

/// Power and standby on one clock (SkateLauncher.cs Assembly / PanelOpen / ScreenLevel): the panel swings out from
/// edge-on and settles with a wobble, then the backlight comes on (sometimes with a flicker); closing plays it
/// backwards, faster. A game start folds the panel shut with the screen still lit, the game's exit swings it back
/// open lit. Every open and close rolls its own swing time, wobble, delay and flicker; snapshots keep the plain values.
pub const ON_TOTAL: f32 = 1.65;
const OFF_SPEED: f32 = 1.3;
pub const OFF_TIME: f32 = ON_TOTAL / OFF_SPEED + 0.05;

pub struct Anim {
    /// Snapshots: this far through the power-on (0..1), fixed.
    pub snap: Option<f32>,
    pub on_at: Option<f32>,
    pub off_at: Option<f32>,
    pub fold_at: Option<f32>,
    pub resume_at: Option<f32>,
    vary: [f32; 5],
    rng: NetRandom,
}

impl Anim {
    pub fn new(seed: i32) -> Self {
        Anim { snap: None, on_at: None, off_at: None, fold_at: None, resume_at: None, vary: [1.0, 0.6, 0.0, 0.0, 0.0], rng: NetRandom::new(seed) }
    }
    /// `Vary`: this open's or close's own swing time, wobble, light delay and flicker.
    pub fn roll(&mut self) {
        let mut u = |lo: f64, hi: f64| (lo + self.rng.next_double() * (hi - lo)) as f32;
        let (a, b, c) = (u(0.85, 1.15), u(0.2, 1.2), u(-0.04, 0.06));
        let flicker = self.rng.next(3) as f32;
        let phase = self.rng.next_double() as f32;
        self.vary = [a, b, c, flicker, phase];
    }
    pub fn standby(&self) -> bool {
        self.fold_at.is_some() || self.resume_at.is_some()
    }
    fn swing_to(&self) -> f32 {
        0.08 + 0.8 * self.vary[0]
    }
    fn closing(&self) -> bool {
        self.snap.is_none() && (self.fold_at.is_some() || self.off_at.is_some())
    }
    pub fn fold_time(&self) -> f32 {
        self.swing_to() / OFF_SPEED + 0.05
    }
    pub fn resume_time(&self) -> f32 {
        self.swing_to() + 0.05
    }
    fn assembly(&self, t: f32) -> f32 {
        if let Some(s) = self.snap { return s * ON_TOTAL; }
        if let Some(f) = self.fold_at { return (self.swing_to() - (t - f) * OFF_SPEED).max(0.0); }
        if let Some(r) = self.resume_at { return (t - r).min(self.swing_to()); }
        if let Some(o) = self.off_at { return (ON_TOTAL - (t - o) * OFF_SPEED).max(0.0); }
        self.on_at.map_or(0.0, |o| (t - o).min(ON_TOTAL))
    }
    /// 0 = shut (edge-on at the hinge), 1 = facing you. A heavy panel (mine, not the classic's BackOut): opening
    /// starts slowly and settles on a damped spring with a small overshoot; closing gathers speed and lands shut with
    /// a small bounce. Doc 44a "Open / close swing".
    pub fn panel_open(&self, t: f32) -> f32 {
        let x = ((self.assembly(t) - 0.08) / (self.swing_to() - 0.08)).clamp(0.0, 1.0);
        if self.closing() { heavy_close(1.0 - x) } else { heavy_open(x, self.vary[1]) }
    }
    pub fn screen_level(&self, t: f32) -> f32 {
        if self.standby() { return 1.0; }
        let mut level = ((self.assembly(t) - self.swing_to() - 0.1 - self.vary[2]) / 0.25).clamp(0.0, 1.0);
        if self.vary[3] >= 1.0 && level > 0.0 && level < 1.0 {
            let q = (level * 6.0 + self.vary[4]) as i32;
            if q % 3 == 1 && q < 3 * self.vary[3] as i32 + 1 { level *= 0.25; }
        }
        level
    }
    /// `WindowAlpha`: the whole window fades in over the first moment of the power-on and out at the end of the
    /// power-off.
    pub fn window_alpha(&self, t: f32) -> f32 {
        if self.snap.is_some() { return 1.0; }
        (self.assembly(t) / 0.06).clamp(0.0, 1.0)
    }

    pub fn animating(&self, t: f32) -> bool {
        self.on_at.is_none_or(|o| t - o < ON_TOTAL + 0.1) || self.off_at.is_some() || self.standby()
    }
}

/// `FlipPanel`: the panel drawn this frame swung on the hinge at its right edge with perspective (the free edge
/// recedes while it is edge-on, comes a touch closer in the wobble past flat), darker while it is turned away.
/// Every shape is tessellated and its vertices moved, so text, screen and wear swing with the face.
fn swing_panel(ctx: &egui::Context, painter: &egui::Painter, hinge_start: egui::layers::ShapeIdx, start: egui::layers::ShapeIdx, open: f32, panel: Rect) {
    let layer = painter.layer_id();
    let (w, h, hx, cy) = (panel.width(), panel.height(), panel.right(), panel.center().y);
    // Shut = the face edge-on to the eye (a little past 90 degrees, since the eye is left of the hinge), so a closed
    // panel shows none of its face, only its side.
    let shut = std::f32::consts::PI - (2.0 * 2.4f32).atan();
    let a = (1.0 - open.max(0.0)) * shut;
    let (cos, sin) = (a.cos(), a.sin());
    // One camera for the face, its side and the shade: the eye in front of the panel's center, `eye_d` back
    // (mine; the classic's eye sits on the hinge line with a 0.6 depth factor). X right, Z away from you.
    let (eye_x, eye_d) = (panel.center().x, w * 2.4); // eye_d / (w / 2) = 2 x 2.4 sets the shut angle above
    let proj = move |x: f32, y: f32, z: f32| { let k = eye_d / (eye_d + z); pos2(eye_x + (x - eye_x) * k, cy + (y - cy) * k) };
    let warp = move |p: Pos2| { let u = (hx - p.x) / w; proj(hx - u * w * cos, p.y, u * w * sin) };
    let ppp = ctx.pixels_per_point();
    let opts = ctx.tessellation_options(|o| *o);
    let font_size = ctx.fonts(|f| f.font_image_size());
    let shapes: Vec<(usize, epaint::ClippedShape)> = ctx.graphics_mut(|g| {
        let list = g.entry(layer);
        list.all_entries().enumerate().skip(start.0).map(|(i, c)| (i, c.clone())).collect()
    });
    let mut tess = epaint::Tessellator::new(ppp, opts, font_size, Vec::new());
    let max_w = w / 32.0;
    let warped: Vec<(usize, Rect, Shape)> = shapes.into_iter().map(|(i, c)| {
        let mut mesh = epaint::Mesh::default();
        tess.set_clip_rect(Rect::EVERYTHING);
        tess.tessellate_shape(c.shape, &mut mesh);
        let mut mesh = subdivide(mesh, max_w);
        for v in &mut mesh.vertices { v.pos = warp(v.pos); }
        let clip = if c.clip_rect.is_finite() && c.clip_rect.width() < 1.0e6 {
            let q = [c.clip_rect.left_top(), c.clip_rect.right_top(), c.clip_rect.left_bottom(), c.clip_rect.right_bottom()].map(warp);
            Rect::from_points(&q)
        } else { c.clip_rect };
        (i, clip, Shape::mesh(mesh))
    }).collect();
    ctx.graphics_mut(|g| {
        let list = g.entry(layer);
        for (i, clip, shape) in warped { list.set(egui::layers::ShapeIdx(i), clip, shape); }
    });
    let dark = (170.0 * sin.max(0.0)) as u8;
    if dark > 0 {
        let q = [panel.left_top(), panel.right_top(), panel.right_bottom(), panel.left_bottom()].map(warp);
        painter.add(Shape::convex_polygon(q.to_vec(), argb(dark, 0, 0, 0), Stroke::NONE));
    }
    if sin > 0.0 {
        // The hinge end of a panel `depth` thick: turned away, it faces you on the right of the front face (its back
        // edge at hinge + depth x (sin, cos)), drawn in columns across the depth: front shell, seam, back shell.
        let depth = w * 0.06;
        let stops = [(0.0, Color32::from_rgb(120, 123, 132)), (0.1, Color32::from_rgb(82, 85, 94)), (0.46, Color32::from_rgb(60, 62, 69)),
                     (0.5, Color32::from_rgb(14, 15, 18)), (0.54, Color32::from_rgb(56, 58, 65)), (1.0, Color32::from_rgb(30, 31, 35))];
        // The right half of the face's outline (PANEL_R's 34 corner radius), extruded back by the depth: the flat side
        // and both rounded corners, so the side meets the face's top and bottom edges.
        let r = 34.0 * w / PANEL_R.width();
        let (top, bottom) = (cy - h / 2.0, cy + h / 2.0);
        let mut outline: Vec<Pos2> = Vec::with_capacity(20);
        for k in 0..=8 { let t = (270.0 + 90.0 * k as f32 / 8.0f32).to_radians(); outline.push(pos2(hx - r + r * t.cos(), top + r + r * t.sin())); }
        for k in 0..=8 { let t = (90.0 * k as f32 / 8.0f32).to_radians(); outline.push(pos2(hx - r + r * t.cos(), bottom - r + r * t.sin())); }
        let grid: Vec<Vec<(Pos2, Color32)>> = outline.iter().map(|p| {
            let u = (hx - p.x) / w;
            let (fx, fz) = (hx - u * w * cos, u * w * sin);
            stops.iter().map(|&(t, c)| (proj(fx + t * depth * sin, p.y, fz + t * depth * cos), lerp(c, Color32::BLACK, dark as f32 / 255.0 * 0.5))).collect()
        }).collect();
        hinge_side(painter, &grid);
    }
    // The hinge barrel stays in front of the panel's side.
    let hinge: Vec<epaint::ClippedShape> = ctx.graphics_mut(|g| g.entry(layer).all_entries().skip(hinge_start.0).take(start.0 - hinge_start.0).cloned().collect());
    for c in hinge { painter.with_clip_rect(c.clip_rect).add(c.shape); }
}

/// The panel's hinge-end side (mine, not in the classic): a grid of points, one row per outline point, one column
/// per depth stop from the face to the back.
fn hinge_side(painter: &egui::Painter, grid: &[Vec<(Pos2, Color32)>]) {
    let (rows, cols) = (grid.len(), grid.first().map_or(0, |r| r.len()));
    if rows < 2 || cols < 2 { return; }
    let mut mesh = egui::Mesh::default();
    for row in grid { for (p, c) in row { mesh.colored_vertex(*p, *c); } }
    for i in 0..rows - 1 {
        for j in 0..cols - 1 {
            let (a, b) = ((i * cols + j) as u32, (i * cols + j + 1) as u32);
            let (c, d) = (((i + 1) * cols + j) as u32, ((i + 1) * cols + j + 1) as u32);
            mesh.add_triangle(a, b, d);
            mesh.add_triangle(a, d, c);
        }
    }
    painter.add(Shape::mesh(mesh));
    let back: Vec<Pos2> = grid.iter().map(|r| r[cols - 1].0).collect();
    painter.add(Shape::line(back, Stroke::new(1.0_f32, argb(150, 0, 0, 0))));
}


/// Splits triangles wider than `max_w` so colors and textures follow the perspective (the classic's 48 strips).
fn subdivide(mesh: epaint::Mesh, max_w: f32) -> epaint::Mesh {
    let mut out = epaint::Mesh::with_texture(mesh.texture_id);
    out.vertices = mesh.vertices;
    let mut stack: Vec<[u32; 3]> = mesh.indices.chunks_exact(3).map(|t| [t[0], t[1], t[2]]).collect();
    while let Some(t) = stack.pop() {
        let p = t.map(|i| out.vertices[i as usize].pos);
        let span = p.iter().map(|q| q.x).fold(f32::MIN, f32::max) - p.iter().map(|q| q.x).fold(f32::MAX, f32::min);
        if span <= max_w || stack.len() > 200_000 { out.indices.extend_from_slice(&t); continue; }
        let e = (0..3).max_by(|&i, &j| p[i].distance(p[(i + 1) % 3]).total_cmp(&p[j].distance(p[(j + 1) % 3]))).unwrap_or(0);
        let (a, b, c) = (t[e], t[(e + 1) % 3], t[(e + 2) % 3]);
        let (va, vb) = (out.vertices[a as usize], out.vertices[b as usize]);
        let m = epaint::Vertex { pos: va.pos.lerp(vb.pos, 0.5), uv: va.uv.lerp(vb.uv, 0.5), color: va.color.lerp_to_gamma(vb.color, 0.5) };
        let mi = out.vertices.len() as u32;
        out.vertices.push(m);
        stack.push([a, mi, c]);
        stack.push([mi, b, c]);
    }
    out
}

/// Travel with constant acceleration over the first `a`, steady speed, constant braking over the last `d` (0..1).
fn trapezoid(k: f32, a: f32, d: f32) -> f32 {
    let k = k.clamp(0.0, 1.0);
    let v = 1.0 / (1.0 - a / 2.0 - d / 2.0);
    if k < a { return v * k * k / (2.0 * a); }
    if k <= 1.0 - d { return v * (a / 2.0 + (k - a)); }
    let r = 1.0 - k;
    1.0 - v * r * r / (2.0 * d)
}

/// Opening over x in 0..1, like a camcorder's flip screen pulled open on its friction hinge: it takes a moment to get
/// going, swings at a steady pace, slows into the stop and clicks into the detent with a small firm settle (the
/// click's size from the roll).
fn heavy_open(x: f32, roll: f32) -> f32 {
    const STOP: f32 = 0.9;
    if x <= 0.0 { return 0.0; }
    if x >= 1.0 { return 1.0; }
    if x < STOP { return trapezoid(x / STOP, 0.25, 0.3); }
    let k = (x - STOP) / (1.0 - STOP);
    let click = 0.008 + 0.006 * ((roll - 0.2) / 1.0).clamp(0.0, 1.0);
    1.0 + click * (k * std::f32::consts::PI).sin() * (1.0 - k)
}

/// Closing over u in 0..1: pushed shut against the hinge's friction, it slows near the body, then the latch pulls it
/// in with a quick snap (shut at u = 0.88) and a small rebound.
fn heavy_close(u: f32) -> f32 {
    const PUSH: f32 = 0.78;
    const HIT: f32 = 0.88;
    if u <= 0.0 { return 1.0; }
    if u < PUSH { return 1.0 - 0.86 * trapezoid(u / PUSH, 0.3, 0.35); }
    if u < HIT { let k = (u - PUSH) / (HIT - PUSH); return 0.14 * (1.0 - k * k); }
    let b = ((u - HIT) / (1.0 - HIT)).clamp(0.0, 1.0);
    0.018 * (b * std::f32::consts::PI).sin() * (1.0 - b)
}


/// Paints the whole window; returns what the pointer hit.
/// Where the screen glass and the hinge line land in a window of this size (points): for `--standby-test`.
pub fn screen_and_hinge(window: Rect) -> (Rect, f32) {
    let dev_s = (window.width() / DEV.x).min(window.height() / DEV.y);
    let dev_x = window.left() + (window.width() - DEV.x * dev_s) / 2.0;
    let dev_y = window.top() + (window.height() - DEV.y * dev_s) / 2.0;
    let r = Rect::from_min_max(pos2(dev_x + SCREEN_R.min.x * dev_s, dev_y + SCREEN_R.min.y * dev_s), pos2(dev_x + SCREEN_R.max.x * dev_s, dev_y + SCREEN_R.max.y * dev_s));
    (r, dev_x + PANEL_R.right() * dev_s)
}

pub fn paint(ui: &mut egui::Ui, view: &View, t: f32, open: f32, level: f32, wear: Option<egui::TextureId>) -> Hits {
    let window = ui.max_rect();
    let dev_s = (window.width() / DEV.x).min(window.height() / DEV.y);
    let dev_x = window.left() + (window.width() - DEV.x * dev_s) / 2.0;
    let dev_y = window.top() + (window.height() - DEV.y * dev_s) / 2.0;
    let hinge = dev_x + 1205.0 * dev_s;
    let xf = Xf { ox: dev_x, oy: dev_y, sx: dev_s, sy: dev_s, swing: 1.0, hinge };
    let painter = ui.painter().clone();
    let c = Ctx { painter: &painter, xf };
    let mut hits = Hits::default();
    PROMPTS.with(|p| p.borrow_mut().clear());
    let pointer = ui.ctx().input(|i| i.pointer.hover_pos());
    let clicked = ui.ctx().input(|i| i.pointer.primary_clicked());
    let pressed = ui.ctx().input(|i| i.pointer.primary_pressed());
    let mut keys: Vec<(Rect, Key)> = Vec::new();
    let mut items: Vec<(Rect, usize)> = Vec::new();

    let hinge_start = ui.ctx().graphics_mut(|g| g.entry(painter.layer_id()).next_idx());
    if open > 0.01 {
        // Hinge (Cylinder upright + grooves).
        let hr = Rect::from_min_size(pos2(1186.0, 150.0), vec2(38.0, 460.0));
        let hx = Xf { swing: 1.0, ..xf };
        let hc = Ctx { painter: &painter, xf: hx };
        let pts = round_pts(hr, 16.0);
        let mut mesh = egui::Mesh::default();
        let stops = [(0.0, PLASTIC_DARK), (0.28, PLASTIC_HI), (0.62, PLASTIC), (1.0, PLASTIC_DARK)];
        let col = |x: f32| { let u = ((x - hr.left() + 1.0) / (hr.width() + 2.0)).clamp(0.0, 1.0); let mut c0 = stops[0]; for s in stops { if u >= s.0 { c0 = s; } } let nxt = stops.iter().find(|s| s.0 > u).copied().unwrap_or(c0); if nxt.0 > c0.0 { lerp(c0.1, nxt.1, (u - c0.0) / (nxt.0 - c0.0)) } else { c0.1 } };
        let cc = hr.center();
        mesh.colored_vertex(hx.pt(cc), col(cc.x));
        for p in &pts { mesh.colored_vertex(hx.pt(*p), col(p.x)); }
        let n = pts.len() as u32;
        for i in 0..n { mesh.add_triangle(0, 1 + i, 1 + (i + 1) % n); }
        painter.add(Shape::mesh(mesh));
        hc.stroke_poly(&pts, 2.0, argb(160, 0, 0, 0));
        let mut y = 200.0;
        while y < 600.0 { hc.line(pos2(1188.0, y), pos2(1222.0, y), 2.0, argb(110, 0, 0, 0)); y += 70.0; }
    }

    let panel_start = ui.ctx().graphics_mut(|g| g.entry(painter.layer_id()).next_idx());
    // LcdPanel.
    let pts = round_pts(PANEL_R, 34.0);
    c.fill_vgrad(&pts, PANEL_R.top() - 1.0, PANEL_R.bottom() + 1.0, Color32::from_rgb(76, 78, 86), Color32::from_rgb(32, 33, 38));
    c.stroke_poly(&pts, 3.0, argb(210, 0, 0, 0));
    c.stroke_poly(&round_pts(grow(PANEL_R, -10.0), 26.0), 2.0, argb(30, 255, 255, 255));
    c.write("SK8-1000  HD", U_MODEL, PRINT, Rect::from_min_size(pos2(822.0, 700.0), vec2(272.0, U_MODEL.height() * 1.3)), Align::Far, Align::Near, false);
    c.fill_ellipse(Rect::from_min_size(pos2(1144.0, 30.0), vec2(12.0, 12.0)), Color32::from_rgb(14, 14, 16));
    c.fill_poly(&round_pts(grow(SCREEN_R, 13.0), 10.0), Color32::from_rgb(8, 8, 10));
    if level > 0.0 {
        lcd_canvas(&c, view, t, &mut items);
        if level < 1.0 { c.fill_rect(SCREEN_R, argb((255.0 * (1.0 - level)) as u8, 6, 7, 9)); }
    } else {
        c.fill_rect(SCREEN_R, Color32::from_rgb(16, 17, 20));
    }
    c.fill_poly(&[SCREEN_R.min, pos2(SCREEN_R.left() + SCREEN_R.width() * 0.42, SCREEN_R.top()), pos2(SCREEN_R.left(), SCREEN_R.top() + SCREEN_R.height() * 0.55)], argb(14, 255, 255, 255));

    let hover = view.hover;
    let hard_key = |r: Rect, key: Key, label: Option<&str>, rad: f32, keys: &mut Vec<(Rect, Key)>| {
        let over = hover == Some(key);
        c.card(r, rad, if over { Color32::from_rgb(132, 136, 146) } else { Color32::from_rgb(92, 95, 104) }, if over { Color32::from_rgb(70, 72, 80) } else { Color32::from_rgb(44, 46, 52) }, argb(170, 0, 0, 0), 2.0);
        if let Some(l) = label { c.write(l, U_KEY, PRINT, r, Align::Center, Align::Center, false); }
        keys.push((r, key));
    };
    let print_at = |s: &str, f: Font, x: f32, y: f32, w: f32, a: Align| c.write(s, f, PRINT, Rect::from_min_size(pos2(x, y), vec2(w, f.height() * 1.3)), a, Align::Near, false);
    hard_key(Rect::from_min_size(pos2(40.0, 70.0), vec2(110.0, 48.0)), Key::Menu, Some("HOME"), 12.0, &mut keys);
    print_at("ZOOM", U_TINY, 30.0, 146.0, 130.0, Align::Center);
    let rocker = Rect::from_min_size(pos2(66.0, 168.0), vec2(58.0, 172.0));
    c.card(rocker, 29.0, Color32::from_rgb(92, 95, 104), Color32::from_rgb(44, 46, 52), argb(170, 0, 0, 0), 2.0);
    let t_half = Rect::from_min_size(rocker.min, vec2(rocker.width(), rocker.height() / 2.0));
    let w_half = Rect::from_min_size(pos2(rocker.left(), rocker.top() + rocker.height() / 2.0), vec2(rocker.width(), rocker.height() / 2.0));
    if hover == Some(Key::Up) { c.card(t_half, 29.0, argb(70, 255, 255, 255), argb(30, 255, 255, 255), Color32::TRANSPARENT, 0.0); }
    if hover == Some(Key::Down) { c.card(w_half, 29.0, argb(70, 255, 255, 255), argb(30, 255, 255, 255), Color32::TRANSPARENT, 0.0); }
    c.line(pos2(rocker.left() + 8.0, rocker.center().y), pos2(rocker.right() - 8.0, rocker.center().y), 2.0, argb(150, 0, 0, 0));
    c.write("T", U_KEY, PRINT, t_half, Align::Center, Align::Center, false);
    c.write("W", U_KEY, PRINT, w_half, Align::Center, Align::Center, false);
    keys.push((t_half, Key::Up));
    keys.push((w_half, Key::Down));
    hard_key(Rect::from_min_size(pos2(40.0, 372.0), vec2(110.0, 48.0)), Key::A, Some("SET"), 12.0, &mut keys);
    print_at("START/STOP", U_TINY, 30.0, 436.0, 130.0, Align::Center);
    c.fill_ellipse(Rect::from_min_size(pos2(39.0, 456.0), vec2(112.0, 112.0)), Color32::from_rgb(26, 27, 31));
    let over_play = hover == Some(Key::Play);
    c.fill_ellipse_vgrad(Rect::from_min_size(pos2(51.0, 468.0), vec2(88.0, 88.0)), if over_play { Color32::from_rgb(255, 120, 110) } else { Color32::from_rgb(240, 70, 60) }, Color32::from_rgb(150, 20, 16));
    c.fill_ellipse(Rect::from_min_size(pos2(70.0, 478.0), vec2(36.0, 20.0)), argb(70, 255, 255, 255));
    keys.push((Rect::from_min_size(pos2(39.0, 456.0), vec2(112.0, 112.0)), Key::Play));
    let power = Rect::from_min_size(pos2(48.0, 588.0), vec2(94.0, 36.0));
    hard_key(power, Key::Power, None, 10.0, &mut keys);
    c.write("POWER", U_TINY, PRINT, power, Align::Center, Align::Center, false);
    for r in 0..4 {
        for col in 0..7 {
            c.fill_ellipse(Rect::from_min_size(pos2(188.0 + col as f32 * 12.0 + (r % 2) as f32 * 6.0, 642.0 + r as f32 * 12.0), vec2(6.0, 6.0)), argb(150, 0, 0, 0));
        }
    }
    print_at("16:9 WIDE LCD", U_TINY, 180.0, 700.0, 160.0, Align::Near);
    hard_key(Rect::from_min_size(pos2(716.0, 640.0), vec2(84.0, 32.0)), Key::Display, None, 8.0, &mut keys);
    print_at("DISPLAY", U_TINY, 716.0, 676.0, 84.0, Align::Center);
    let tape_keys = [Key::Left, Key::A, Key::Right, Key::Menu];
    let names = ["REW", "PLAY", "FF", "STOP"];
    for i in 0..4 {
        let r = Rect::from_min_size(pos2(826.0 + i as f32 * 66.0, 640.0), vec2(56.0, 32.0));
        hard_key(r, tape_keys[i], None, 8.0, &mut keys);
        tape_glyph(&c, i, r);
        print_at(names[i], U_TINY, r.left() - 6.0, 676.0, r.width() + 12.0, Align::Center);
    }
    // Wear and sticker (wear.rs) over the bezel.
    if let Some(tex) = wear {
        painter.image(tex, xf.r(Rect::from_min_size(pos2(0.0, 0.0), DEV)), Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)), Color32::WHITE);
    }
    if (open - 1.0).abs() >= 0.002 {
        swing_panel(ui.ctx(), &painter, hinge_start, panel_start, open, xf.r(PANEL_R));
    }

    // Hits.
    for (r, k) in &keys {
        if pointer.is_some_and(|p| xf.r(*r).contains(p)) {
            hits.hover = Some(*k);
            if clicked { hits.key = Some(*k); }
        }
    }
    for (r, i) in &items {
        if pointer.is_some_and(|p| xf.r(*r).contains(p)) {
            hits.hover_item = Some(*i);
            if clicked { hits.item = Some(*i); }
        }
    }
    hits.right_click = ui.ctx().input(|i| i.pointer.secondary_clicked());
    if let Some(p) = pointer {
        hits.prompt_hover = PROMPTS.with(|v| v.borrow().iter().find(|(r, _)| r.contains(p)).map(|(_, k)| k.clone()));
        if clicked { hits.prompt = hits.prompt_hover.clone(); }
    }
    hits.empty_click = clicked && hits.item.is_none() && hits.key.is_none() && hits.prompt.is_none();
    let body = xf.r(PANEL_R);
    if pressed && hits.hover.is_none() && hits.item.is_none() && pointer.is_some_and(|p| body.contains(p)) {
        hits.drag = true;
    }
    hits
}

fn tape_glyph(c: &Ctx, kind: usize, r: Rect) {
    let (cx, cy, s) = (r.center().x, r.center().y, 8.0);
    let tri = |a: (f32, f32), b: (f32, f32), d: (f32, f32)| c.fill_poly(&[pos2(a.0, a.1), pos2(b.0, b.1), pos2(d.0, d.1)], PRINT);
    match kind {
        0 => { tri((cx - 1.0, cy), (cx + 9.0, cy - s), (cx + 9.0, cy + s)); tri((cx - 11.0, cy), (cx - 1.0, cy - s), (cx - 1.0, cy + s)); }
        1 => tri((cx + 9.0, cy), (cx - 6.0, cy - s), (cx - 6.0, cy + s)),
        2 => { tri((cx + 1.0, cy), (cx - 9.0, cy - s), (cx - 9.0, cy + s)); tri((cx + 11.0, cy), (cx + 1.0, cy - s), (cx + 1.0, cy + s)); }
        _ => c.fill_rect(Rect::from_min_size(pos2(cx - 7.0, cy - 7.0), vec2(14.0, 14.0)), PRINT),
    }
}

/// `LcdCanvas`: the screen's content in a 340-wide layout space over the scene.
fn lcd_canvas(panel: &Ctx, view: &View, t: f32, items: &mut Vec<(Rect, usize)>) {
    let w = 340.0f32;
    let h = (340.0 * SCREEN_R.height() / SCREEN_R.width()).round();
    let k = SCREEN_R.width() / w;
    let xf = Xf { ox: panel.xf.ox + SCREEN_R.left() * panel.xf.sx, oy: panel.xf.oy + SCREEN_R.top() * panel.xf.sy, sx: panel.xf.sx * k, sy: panel.xf.sy * (SCREEN_R.height() / h), swing: panel.xf.swing, hinge: panel.xf.hinge };
    let clip = panel.xf.r(SCREEN_R);
    let painter = panel.painter.with_clip_rect(clip);
    let o = Ctx { painter: &painter, xf };
    o.fill_rect(Rect::from_min_size(pos2(0.0, 0.0), vec2(w, h)), Color32::from_rgb(6, 8, 14));
    let osd = Osd::new(h);
    if view.splash.is_none() {
        scene(&o, w, h);
    }
    let foot = osd.small.height() + 4.0;
    let lane = 15.0;
    let top = osd.row.height() + 7.0;
    let bottom = h - foot - lane - 2.0;
    if let Some(stage) = view.splash {
        lcd_splash(&o, &osd, w, h, t, stage, view.splash_secs);
    } else {
        lcd_status(&o, &osd, w, view);
        if let Some(job) = &view.job {
            lcd_job(&o, &osd, w, top, bottom, t, job);
        } else {
            let mut local = Vec::new();
            lcd_page(&o, &osd, view, w, top, bottom, t, &mut local);
            for (r, i) in local {
                let u = Rect::from_min_size(pos2(SCREEN_R.left() + r.left() * SCREEN_R.width() / w, SCREEN_R.top() + r.top() * SCREEN_R.height() / h), vec2(r.width() * SCREEN_R.width() / w, r.height() * SCREEN_R.height() / h));
                items.push((u, i));
            }
        }
    }
    lcd_board(&o, t, h - foot - 2.0, w);
    lcd_foot(&o, &osd, view, w, h);
}

/// `Scene`: a skate spot at dusk (sun, skyline, stairs with a handrail, a ledge), dimmed under the menus.
fn scene(o: &Ctx, w: f32, h: f32) {
    let hz = h * 0.62;
    o.card(Rect::from_min_size(pos2(0.0, 0.0), vec2(w, hz)), 0.0, Color32::from_rgb(34, 46, 104), Color32::from_rgb(222, 128, 78), Color32::TRANSPARENT, 0.0);
    o.fill_ellipse(Rect::from_min_size(pos2(w * 0.68, hz - h * 0.17), vec2(h * 0.16, h * 0.16)), argb(200, 252, 210, 136));
    let mut rnd = NetRandom::new(5);
    let mut x = 0.0;
    while x < w {
        let bw = w * (0.024 + rnd.next_double() as f32 * 0.045);
        let bh = h * (0.05 + rnd.next_double() as f32 * 0.18);
        o.fill_rect(Rect::from_min_size(pos2(x, hz - bh), vec2(bw, bh)), Color32::from_rgb(44, 34, 56));
        x += bw + 1.0;
    }
    o.card(Rect::from_min_size(pos2(0.0, hz), vec2(w, h - hz)), 0.0, Color32::from_rgb(82, 68, 70), Color32::from_rgb(30, 26, 30), Color32::TRANSPARENT, 0.0);
    let (sx0, sy0, step_w, step_h) = (w * 0.6, h * 0.7, w * 0.08, h * 0.06);
    let mut stairs = vec![pos2(sx0, h), pos2(sx0, sy0)];
    for i in 0..5 {
        stairs.push(pos2(sx0 + (i + 1) as f32 * step_w, sy0 + i as f32 * step_h));
        stairs.push(pos2(sx0 + (i + 1) as f32 * step_w, sy0 + (i + 1) as f32 * step_h));
    }
    stairs.push(pos2(w, sy0 + 5.0 * step_h));
    stairs.push(pos2(w, h));
    // Not convex: fill it as rectangles step by step.
    for i in 0..5 {
        let x0 = sx0 + i as f32 * step_w;
        o.fill_rect(Rect::from_min_max(pos2(x0, sy0 + i as f32 * step_h), pos2(x0 + step_w, h)), Color32::from_rgb(54, 48, 54));
    }
    o.fill_rect(Rect::from_min_max(pos2(sx0 + 5.0 * step_w, sy0 + 5.0 * step_h), pos2(w, h)), Color32::from_rgb(54, 48, 54));
    let lw = (h / 190.0).max(1.0);
    for i in 0..5 {
        o.line(pos2(sx0 + i as f32 * step_w, sy0 + i as f32 * step_h), pos2(sx0 + (i + 1) as f32 * step_w, sy0 + i as f32 * step_h), lw, argb(130, 222, 156, 112));
    }
    o.fill_rect(Rect::from_min_size(pos2(w * 0.05, h * 0.74), vec2(w * 0.3, h * 0.07)), Color32::from_rgb(74, 68, 70));
    o.line(pos2(w * 0.05, h * 0.74), pos2(w * 0.35, h * 0.74), lw, argb(150, 222, 156, 112));
    let rail = argb(170, 176, 178, 190);
    let y_at = |px: f32| sy0 - h * 0.09 + (px - sx0 - step_w * 0.5) * step_h / step_w;
    o.line(pos2(sx0 + step_w * 0.5, sy0 - h * 0.09), pos2(w, y_at(w)), lw * 1.4, rail);
    for i in 0..4 {
        let px = sx0 + step_w * (0.5 + i as f32 * 1.2);
        o.line(pos2(px, y_at(px)), pos2(px, sy0 + i as f32 * 1.2 * step_h), lw * 1.4, rail);
    }
    o.fill_rect(Rect::from_min_size(pos2(0.0, 0.0), vec2(w, h)), argb(86, 0, 0, 0));
    // The vignette outside the big ellipse: darkened corners.
    let ring: Vec<Pos2> = (0..48).map(|i| { let a = i as f32 / 48.0 * std::f32::consts::TAU; pos2(w * 0.5 + w * 0.7 * a.cos(), h * 0.5 + h * 0.75 * a.sin()) }).collect();
    let mut mesh = egui::Mesh::default();
    for (i, p) in ring.iter().enumerate() {
        let outer = pos2(w * 0.5 + (p.x - w * 0.5) * 2.0, h * 0.5 + (p.y - h * 0.5) * 2.0);
        mesh.colored_vertex(o.xf.pt(*p), argb(70, 0, 0, 0));
        mesh.colored_vertex(o.xf.pt(outer), argb(70, 0, 0, 0));
        let n = ring.len() as u32;
        let (a, b) = (2 * i as u32, 2 * ((i as u32 + 1) % n));
        mesh.add_triangle(a, a + 1, b);
        mesh.add_triangle(a + 1, b + 1, b);
    }
    o.painter.add(Shape::mesh(mesh));
}

fn pill(o: &Ctx, osd: &Osd, s: &str, c: Color32, x: f32, y: f32) {
    let r = Rect::from_min_size(pos2(x, y), vec2(o.measure(s, osd.tiny) + 3.0, osd.tiny.height() + 2.0));
    let pts = round_pts(r, r.height() / 2.0);
    o.fill_poly(&pts, rgba(46, c));
    o.stroke_poly(&pts, 0.7, rgba(200, c));
    o.write(s, osd.tiny, c, r, Align::Center, Align::Center, false);
}

/// `LcdStatus`: STBY / REC with its dot, the HDV badge, the tape counter, the battery.
fn lcd_status(o: &Ctx, osd: &Osd, w: f32, view: &View) {
    let (mut x, y, fh) = (5.0, 3.0, osd.row.height());
    let ms = (view.tape.fract() * 1000.0) as u32;
    if view.recording {
        let d = fh * 0.46;
        let dy = y + (fh - d) / 2.0;
        if (jiff::Timestamp::now().subsec_millisecond()) < 650 {
            o.fill_ellipse(Rect::from_min_size(pos2(x - d * 0.35, dy - d * 0.35), vec2(d * 1.7, d * 1.7)), rgba(80, REC_RED));
            o.fill_ellipse(Rect::from_min_size(pos2(x, dy), vec2(d, d)), REC_RED);
        }
        x += d + 3.0;
    }
    let mode = if view.recording { "REC" } else { "STBY" };
    o.shadow_text(mode, osd.row, if view.recording { LCD_RED } else { LCD_INK }, x, y);
    x += o.measure(mode, osd.row) + 3.0;
    let hs = o.measure("HDV 1080i", osd.tiny);
    let hb = Rect::from_min_size(pos2(x, y + (fh - osd.tiny.height() - 1.0) / 2.0), vec2(hs + 2.0, osd.tiny.height() + 1.0));
    let pts = round_pts(hb, 1.5);
    o.fill_poly(&pts, argb(70, 0, 0, 0));
    o.stroke_poly(&pts, 0.7, argb(210, 255, 255, 255));
    o.write("HDV 1080i", osd.tiny, LCD_INK, hb, Align::Center, Align::Center, false);
    x += hb.width() + 6.0;
    let tape = view.tape as u32;
    o.shadow_text(&format!("{}:{:02}:{:02}:{:02}", tape / 3600, tape / 60 % 60, tape % 60, ms * 30 / 1000), osd.row, LCD_INK, x, y);
    let mins = "120 min";
    let msw = o.measure(mins, osd.small);
    let (bw, bh) = (fh * 1.2, fh * 0.52);
    let (bx, by) = (w - 5.0 - msw - bw - 3.0, y + (fh - bh) / 2.0);
    let body = Rect::from_min_size(pos2(bx, by), vec2(bw, bh));
    let pts = round_pts(body, 1.5);
    o.fill_poly(&pts, argb(90, 0, 0, 0));
    o.stroke_poly(&pts, 0.8, argb(220, 255, 255, 255));
    o.fill_rect(Rect::from_min_size(pos2(bx + bw, by + bh * 0.3), vec2(1.4, bh * 0.4)), argb(220, 255, 255, 255));
    let seg = (bw - 2.4) / 4.0;
    for i in 0..4 {
        o.gloss(Rect::from_min_size(pos2(bx + 1.2 + i as f32 * seg + 0.3, by + 1.2), vec2(seg - 0.6, bh - 2.4)), 0.6, Color32::from_rgb(170, 240, 110), Color32::from_rgb(66, 168, 40));
    }
    o.shadow_text(mins, osd.small, LCD_INK, w - 5.0 - msw, y + (fh - osd.small.height()) / 2.0);
}

thread_local! {
    /// The prompts drawn this paint, in window points (SkateLauncher.cs promptHits).
    static PROMPTS: std::cell::RefCell<Vec<(Rect, String)>> = const { std::cell::RefCell::new(Vec::new()) };
}

fn badge(o: &Ctx, osd: &Osd, x: &mut f32, y: f32, key: &str, c: Color32, label: &str) {
    let x0 = *x;
    let d = osd.small.height() * 0.9;
    let r = if key.chars().count() == 1 { Rect::from_min_size(pos2(*x, y + (osd.small.height() - d) / 2.0), vec2(d, d)) } else { Rect::from_min_size(pos2(*x, y + (osd.small.height() - d) / 2.0), vec2(o.measure(key, osd.tiny) + 3.0, d)) };
    o.gloss(r, d / 2.0, mix(c, Color32::WHITE, 0.25), mix(c, Color32::BLACK, 0.2));
    o.write(key, osd.tiny, Color32::WHITE, r, Align::Center, Align::Center, false);
    *x = r.right() + 2.0;
    o.shadow_text(label, osd.small, LCD_INK, *x, y);
    *x += o.measure(label, osd.small) + 5.0;
    let area = o.xf.r(Rect::from_min_max(pos2(x0, y), pos2(*x, y + osd.small.height())));
    PROMPTS.with(|p| p.borrow_mut().push((area, key.to_owned())));
}

/// `LcdFoot`: the keys on the left, the date and time on the right.
fn lcd_foot(o: &Ctx, osd: &Osd, view: &View, w: f32, h: f32) {
    let y = h - osd.small.height() - 3.0;
    let mut x = 5.0;
    let a = Color32::from_rgb(92, 186, 72);
    let b = Color32::from_rgb(220, 70, 60);
    let cancel = view.job.as_ref().is_some_and(|j| (j.waiting.is_some() && j.ended.is_none()) || j.fetch_wait.is_some());
    if let Some(s) = view.close_in {
        o.shadow_text(&format!("Closing in {s} s"), osd.small, LCD_INK, x, y);
    } else if cancel {
        badge(o, osd, &mut x, y, "B", b, if view.cancel_armed { "Again to cancel" } else { "Cancel" });
    } else if view.splash.is_some() {
        if let Some(armed) = view.splash_skip { badge(o, osd, &mut x, y, "B", b, if armed { "Again to skip" } else { "Skip the check" }); }
    } else if view.job.is_none() {
        badge(o, osd, &mut x, y, "A", a, "Select");
        if view.main { badge(o, osd, &mut x, y, "START/STOP", REC_RED, "Play"); } else { badge(o, osd, &mut x, y, "B", b, "Back"); }
    }
    let stamp = jiff::Zoned::now().strftime("%b %-d %Y   %-I:%M %p").to_string().to_uppercase();
    o.shadow_text(&stamp, osd.small, LCD_INK, w - 5.0 - o.measure(&stamp, osd.small), y);
}

/// `Board`: the side-view skateboard, 180 units long, its top-left at (x, y), `len` long, tilted by `tilt` degrees.
fn board(o: &Ctx, x: f32, y: f32, len: f32, tilt: f32) {
    let l = 180.0;
    let k = len / 180.0;
    let (px, py) = (x + len / 2.0, y + len * 17.0 / 180.0);
    let (sn, cs) = tilt.to_radians().sin_cos();
    let tr = |bx: f32, by: f32| { let (wx, wy) = (x + bx * k - px, y + by * k - py); pos2(px + wx * cs - wy * sn, py + wx * sn + wy * cs) };
    let top: [(f32, f32); 12] = [(1.0, 3.0), (5.0, 3.6), (11.0, 5.6), (18.0, 8.6), (25.0, 11.0), (31.0, 12.0), (l - 33.0, 12.0), (l - 26.0, 10.8), (l - 18.0, 7.8), (l - 10.0, 4.4), (l - 4.0, 2.4), (l - 1.0, 2.0)];
    // The deck is not convex: draw it as quads between the upper and lower lines, segment by segment.
    let band = |upper: f32, lower: f32, c: Color32| {
        for i in 0..11 {
            let q = [tr(top[i].0, top[i].1 + upper), tr(top[i + 1].0, top[i + 1].1 + upper), tr(top[i + 1].0, top[i + 1].1 + lower), tr(top[i].0, top[i].1 + lower)];
            o.fill_poly(&q, c);
        }
    };
    o.fill_ellipse(Rect::from_min_size(tr(10.0, 32.5), vec2((l - 20.0) * k, 5.0 * k)), argb(110, 0, 0, 0));
    band(0.0, 5.0, Color32::from_rgb(214, 176, 124));
    band(2.6, 3.4, Color32::from_rgb(150, 104, 62));
    band(0.0, 1.4, Color32::from_rgb(34, 34, 38));
    for ax in [40.0, l - 42.0] {
        o.fill_poly(&[tr(ax - 10.0, 17.0), tr(ax + 10.0, 17.0), tr(ax + 9.0, 19.5), tr(ax - 9.0, 19.5)], Color32::from_rgb(176, 181, 191));
        o.fill_poly(&[tr(ax - 6.0, 19.5), tr(ax + 6.0, 19.5), tr(ax + 3.5, 26.0), tr(ax - 3.5, 26.0)], Color32::from_rgb(176, 181, 191));
        for (c, r) in [(Color32::from_rgb(168, 162, 148), 7.5), (Color32::from_rgb(240, 235, 220), 6.9), (Color32::from_rgb(86, 146, 214), 3.9), (Color32::from_rgb(52, 54, 60), 1.7)] {
            let ctr = tr(ax, 26.5);
            o.fill_ellipse(Rect::from_center_size(ctr, vec2(2.0 * r * k, 2.0 * r * k)), c);
        }
    }
}

/// `LcdBoard`: the board rides a lane along the bottom and pops an ollie now and then (snapshot rhythm).
fn lcd_board(o: &Ctx, t: f32, ground: f32, w: f32) {
    let len = 44.0;
    let (speed, offset, rhythm) = (150.0, 0.0, 6.5);
    let x = -len + (t * speed * 0.25 + offset) % (w + len);
    let cycle = t % rhythm;
    let (mut jump, mut tilt) = (0.0, 0.0);
    if cycle < 0.8 {
        let q = cycle / 0.8;
        jump = (std::f32::consts::PI * q).sin() * 15.0;
        tilt = if q < 0.25 { -18.0 * q / 0.25 } else { -18.0 * (1.0 - (q - 0.25) / 0.75) };
    }
    o.line(pos2(5.0, ground + 0.5), pos2(w - 5.0, ground + 0.5), 0.6, argb(46, 255, 255, 255));
    o.fill_ellipse(Rect::from_min_size(pos2(x + len * 0.1 + jump * 0.3, ground - 1.2), vec2(len * 0.8 - jump * 0.6, 2.6)), argb((110.0 * (1.0 - jump / 24.0)).max(0.0) as u8, 0, 0, 0));
    let y = ground - len * 34.0 / 180.0 - jump;
    board(o, x, y, len, tilt);
}

/// `ShortName`: a button's short label.
pub fn short_name(name: &str) -> String {
    let (current, name) = match name.strip_prefix("> ") { Some(n) => ("> ", n), None => ("", name) };
    let short = match name {
        "Update and build the selected version" => Some("Update and build"),
        "Clear builds and build caches" => Some("Clear builds"),
        "Send the logs now" => Some("Send logs"),
        "Recomp: get the newest build" => Some("Recomp: newest build"),
        "Recomp: get the main PC save" => Some("Recomp: main PC save"),
        _ => None,
    };
    if let Some(s) = short {
        return format!("{current}{s}");
    }
    for lead in ["Play + ", "Play with the ", "Play with "] {
        if let Some(rest) = name.strip_prefix(lead).filter(|r| !r.is_empty()) {
            let mut ch = rest.chars();
            let first = ch.next().map(|c| c.to_uppercase().collect::<String>()).unwrap_or_default();
            return format!("{current}{first}{}", ch.as_str());
        }
    }
    let name = match name.find(" (") { Some(i) if name.ends_with(')') => &name[..i], _ => name };
    format!("{current}{name}")
}

/// `LcdPage`: the title tab, the list with the amber bar, the info beside it, or a text page's glass box.
#[allow(clippy::too_many_arguments)]
fn lcd_page(o: &Ctx, osd: &Osd, view: &View, w: f32, top: f32, bottom: f32, t: f32, hits: &mut Vec<(Rect, usize)>) {
    let m = 5.0;
    let title = if view.main { "HOME".to_owned() } else { view.title.trim_end_matches([':', ' ']).to_owned() };
    let tab = Rect::from_min_size(pos2(m, top), vec2((w - 2.0 * m).min(o.measure(&title, osd.row) + 10.0), osd.row.height() + 3.0));
    o.gloss(tab, tab.height() / 2.0, argb(240, 88, 152, 242), argb(240, 26, 74, 168));
    o.write(&title, osd.row, Color32::WHITE, tab, Align::Center, Align::Center, false);
    let mut y = tab.bottom() + 3.0;
    if let Some(body) = &view.body {
        let br = Rect::from_min_size(pos2(m, y), vec2(w - 2.0 * m, if !view.items.is_empty() { (bottom - y) * 0.6 } else { bottom - y }));
        o.glass(br, 5.0);
        let mut text_h = br.height() - 6.0;
        if view.mono.is_some() { text_h = text_h.min(o.measure_h(body, osd.small, br.width() - 10.0) + 2.0); }
        o.write(body, osd.small, LCD_INK, Rect::from_min_size(pos2(br.left() + 5.0, br.top() + 3.0), vec2(br.width() - 10.0, text_h)), Align::Near, Align::Near, true);
        if let Some(mono) = &view.mono {
            let eb = Rect::from_min_max(pos2(br.left() + 4.0, br.top() + 4.0 + text_h), pos2(br.right() - 4.0, br.bottom() - 4.0));
            if eb.height() > osd.small.height() {
                let pts = round_pts(eb, 3.0);
                o.fill_poly(&pts, argb(190, 70, 14, 18));
                o.stroke_poly(&pts, 0.7, rgba(200, LCD_RED));
                o.write(mono, osd.small, LCD_INK, Rect::from_min_size(pos2(eb.left() + 4.0, eb.top() + 2.0), vec2(eb.width() - 8.0, eb.height() - 4.0)), Align::Near, Align::Near, true);
            }
        }
        y = br.bottom() + 3.0;
    }
    if view.items.is_empty() { return; }
    let split = view.body.is_none();
    let list_w = if split { (w * 0.42).floor() } else { w - 2.0 * m };
    let row_h = osd.row.height() + 4.0;
    let fit = (((bottom - y - 5.0) / row_h) as usize).max(1);
    let n = view.items.len();
    let first = (view.sel as i64 - fit as i64 / 2).min(n as i64 - fit as i64).max(0) as usize;
    let shown = fit.min(n - first);
    let bx = Rect::from_min_size(pos2(m, y), vec2(list_w, shown as f32 * row_h + 5.0));
    o.glass(bx, 5.0);
    for i in first..first + shown {
        let (text, on) = view.items[i];
        let sel = i == view.sel;
        let r = Rect::from_min_size(pos2(bx.left() + 2.5, bx.top() + 2.5 + (i - first) as f32 * row_h), vec2(bx.width() - 5.0, row_h));
        if sel { o.gloss(r, 3.0, LCD_AMBER_HI, LCD_AMBER_LO); }
        let mut label = if split { short_name(text) } else { text.to_owned() };
        let cur = label.starts_with("> ");
        if cur { label = label[2..].to_owned(); }
        let color = if sel { Color32::from_rgb(36, 22, 6) } else if on { LCD_INK } else { Color32::from_rgb(120, 128, 144) };
        o.write(&label, osd.row, color, Rect::from_min_size(pos2(r.left() + 5.0, r.top()), vec2(r.width() - if cur { 14.0 } else { 7.0 }, r.height())), Align::Near, Align::Center, false);
        if cur { o.fill_ellipse(Rect::from_min_size(pos2(r.right() - 8.0, r.top() + row_h / 2.0 - 2.0), vec2(4.0, 4.0)), if sel { Color32::from_rgb(36, 22, 6) } else { LCD_AMBER }); }
        hits.push((r, i));
    }
    let ax = bx.right() - 9.0;
    let pen = argb(200, 255, 255, 255);
    if first > 0 { o.lines(&[pos2(ax, bx.top() + 3.0), pos2(ax + 3.0, bx.top() + 0.5), pos2(ax + 6.0, bx.top() + 3.0)], 1.0, pen); }
    if first + shown < n { o.lines(&[pos2(ax, bx.bottom() - 3.0), pos2(ax + 3.0, bx.bottom() - 0.5), pos2(ax + 6.0, bx.bottom() - 3.0)], 1.0, pen); }
    if split {
        lcd_info(o, osd, view, Rect::from_min_size(pos2(bx.right() + 4.0, y), vec2(w - m - bx.right() - 4.0, bottom - y)), t);
    }
}

/// `LcdInfo`: the selected row's details; on the main screen's Play row, the version and the play prompt.
fn lcd_info(o: &Ctx, osd: &Osd, view: &View, r: Rect, t: f32) {
    o.glass(r, 5.0);
    let (x, w, mut y, bottom, sh) = (r.left() + 6.0, r.width() - 12.0, r.top() + 4.0, r.bottom() - 4.0, osd.small.height());
    let Some((text, on)) = view.items.get(view.sel).copied() else { return };
    if view.main && view.sel == 0 {
        o.write("NOW SELECTED", osd.tiny, LCD_AMBER, Rect::from_min_size(pos2(x, y), vec2(w, osd.tiny.height() + 2.0)), Align::Near, Align::Near, false);
        y += osd.tiny.height() + 1.0;
        let vh = o.measure_h(view.version, osd.big, w).min(osd.big.height() * 2.2);
        o.write(view.version, osd.big, LCD_INK, Rect::from_min_size(pos2(x, y), vec2(w, vh)), Align::Near, Align::Near, true);
        y += vh + 1.0;
        o.write(view.version_status, osd.small, LCD_INK, Rect::from_min_size(pos2(x, y), vec2(w, sh * 2.2)), Align::Near, Align::Near, true);
        y += sh * 2.2;
        let py = bottom - osd.small.height();
        let send = if view.send.is_empty() || view.send == "off" { String::new() } else { format!("   Send: {}", view.send) };
        o.write(&format!("Last session: {}{send}", view.last), osd.small, LCD_DIM, Rect::from_min_max(pos2(x, y), pos2(x + w, py - 4.0)), Align::Near, Align::Near, true);
        o.line(pos2(x, py - 2.5), pos2(x + w, py - 2.5), 0.5, argb(40, 255, 255, 255));
        let pulse = 0.55 + 0.45 * (t * 3.2).sin();
        let d = osd.small.height() * 0.5;
        let mut px = x;
        o.fill_ellipse(Rect::from_min_size(pos2(px - d * 0.3, py + (osd.small.height() - d) / 2.0 - d * 0.3), vec2(d * 1.6, d * 1.6)), rgba((70.0 * pulse) as u8, REC_RED));
        o.fill_ellipse(Rect::from_min_size(pos2(px, py + (osd.small.height() - d) / 2.0), vec2(d, d)), rgba((140.0 + 115.0 * pulse) as u8, REC_RED));
        px += d + 3.0;
        badge(o, osd, &mut px, py, "START/STOP", REC_RED, "to play");
        return;
    }
    let name = text.strip_prefix("> ").unwrap_or(text);
    let nh = o.measure_h(name, osd.row, w).min(osd.row.height() * 2.2);
    o.write(name, osd.row, LCD_INK, Rect::from_min_size(pos2(x, y), vec2(w, nh)), Align::Near, Align::Near, true);
    y += nh + 2.0;
    pill(o, osd, if on { "READY" } else { "NOT AVAILABLE" }, if on { LCD_GREEN } else { LCD_RED }, x, y);
    y += osd.tiny.height() + 5.0;
    let info = if !view.info.is_empty() { view.info } else { view.hint };
    if !info.is_empty() {
        let ih = o.measure_h(info, osd.small, w).min(bottom - y);
        o.write(info, osd.small, LCD_INK, Rect::from_min_size(pos2(x, y), vec2(w, ih)), Align::Near, Align::Near, true);
        y += ih + 2.0;
    }
    let name_w = w * 0.38;
    for rec in view.records {
        if y + sh > bottom {
            o.write("...", osd.small, LCD_DIM, Rect::from_min_size(pos2(x, y - 2.0), vec2(w, sh + 2.0)), Align::Near, Align::Near, false);
            break;
        }
        let (a, b) = rec.split_once(": ").unwrap_or((rec, ""));
        if let Some(head) = a.strip_prefix('#') {
            let line = format!("{}{}", head.to_uppercase(), if b.is_empty() { String::new() } else { format!("  {b}") });
            o.write(&line, osd.tiny, LCD_AMBER, Rect::from_min_size(pos2(x, y + 1.0), vec2(w, osd.tiny.height() + 2.0)), Align::Near, Align::Near, false);
            y += osd.tiny.height() + 3.0;
            continue;
        }
        let rh = o.measure_h(a, osd.small, name_w).max(o.measure_h(b, osd.small, w - name_w - 3.0)).min(sh.max(bottom - y));
        o.line(pos2(x, y), pos2(x + w, y), 0.5, argb(40, 255, 255, 255));
        o.write(a, osd.small, LCD_INK, Rect::from_min_size(pos2(x, y), vec2(name_w, rh)), Align::Near, Align::Near, true);
        o.write(b, osd.small, LCD_DIM, Rect::from_min_size(pos2(x + name_w + 3.0, y), vec2(w - name_w - 3.0, rh)), Align::Near, Align::Near, true);
        y += rh + 1.0;
    }
}

fn clock(secs: f32) -> String {
    let s = secs as u32;
    if s >= 3600 { format!("{}:{:02}:{:02}", s / 3600, s / 60 % 60, s % 60) } else { format!("{}:{:02}", s / 60, s % 60) }
}

/// `LcdJob`: the job's name and time, the progress bar, the counts, the first error or the newest lines.
#[allow(clippy::too_many_arguments)]
fn lcd_job(o: &Ctx, osd: &Osd, w: f32, top: f32, bottom: f32, t: f32, job: &JobView) {
    let m = 5.0;
    let card = Rect::from_min_size(pos2(m, top), vec2(w - 2.0 * m, bottom - top));
    o.glass(card, 6.0);
    let (x, ww, mut y, sh) = (card.left() + 6.0, card.width() - 12.0, card.top() + 4.0, osd.small.height());
    let is_build = job.building.is_some() || job.ended.is_some();
    let failed = job.ended == Some(false);
    let k = match job.progress { Some((a, b)) if is_build && b > 0 => (a as f32 / b as f32).min(1.0), _ => -1.0 };
    o.write(job.building.unwrap_or(job.title), osd.big, LCD_INK, Rect::from_min_size(pos2(x, y), vec2(ww * 0.78, osd.big.height() + 2.0)), Align::Near, Align::Near, false);
    o.write(&clock(job.elapsed), osd.row, LCD_DIM, Rect::from_min_size(pos2(x, y + 1.0), vec2(ww, osd.big.height() + 2.0)), Align::Far, Align::Near, false);
    y += osd.big.height() + 1.0;
    if is_build {
        o.write(&format!("at {}", job.commit), osd.small, LCD_DIM, Rect::from_min_size(pos2(x, y), vec2(ww, sh + 2.0)), Align::Near, Align::Near, false);
        y += sh + 2.0;
    }
    let bar = Rect::from_min_size(pos2(x, y), vec2(ww, 6.0));
    let pts = round_pts(bar, 3.0);
    o.fill_poly(&pts, argb(150, 0, 0, 0));
    o.stroke_poly(&pts, 0.6, argb(60, 255, 255, 255));
    let (hi, lo) = if failed { (Color32::from_rgb(255, 130, 120), Color32::from_rgb(200, 40, 30)) } else { (LCD_AMBER_HI, LCD_AMBER_LO) };
    if k >= 0.0 {
        if k > 0.0 { o.gloss(Rect::from_min_size(bar.min, vec2((bar.width() * k).max(bar.height()), bar.height())), 3.0, hi, lo); }
    } else {
        let run = (t * 0.45) % 1.0;
        let seg_x = bar.left() + run * bar.width() * 1.3 - bar.width() * 0.3;
        let seg = Rect::from_min_size(pos2(seg_x, bar.top()), vec2(bar.width() * 0.3, bar.height())).intersect(bar);
        if seg.width() > 1.0 { o.gloss(seg, 3.0, hi, lo); }
    }
    y += 10.0;
    if is_build {
        let mut count = match job.progress { Some((a, b)) if b > 0 => format!("{a} / {b} build steps"), _ => format!("{} crates compiled", job.compiled) };
        if job.ended.is_none() { if let Some(wt) = job.waiting { count = format!("Waiting {}: {wt}", clock(job.wait_secs)); } else if job.progress.is_none() && job.compiled == 0 && !job.stage.is_empty() { count = job.stage.to_owned(); } }
        if let Some(ok) = job.ended { count = if ok { format!("Built in {}", clock(job.built_secs.unwrap_or(job.elapsed))) } else { "Build failed".into() }; }
        o.write(&count, osd.row, if failed { LCD_RED } else { LCD_INK }, Rect::from_min_size(pos2(x, y), vec2(ww * 0.7, osd.row.height() + 2.0)), Align::Near, Align::Near, false);
        if job.warnings > 0 {
            let wt = format!("{} {}", job.warnings, if job.warnings == 1 { "warning" } else { "warnings" });
            pill(o, osd, &wt, LCD_AMBER, x + ww - o.measure(&wt, osd.tiny) - 3.0, y + 1.0);
        }
        y += osd.row.height() + 2.0;
        let line = if job.ended.is_none() && job.waiting.is_some() { job.wait_who.unwrap_or("Cargo does not say who; it continues by itself once that build ends.").to_owned() } else if !job.now.is_empty() && job.ended.is_none() { format!("Now: {}", job.now) } else { String::new() };
        if !line.is_empty() {
            o.write(&line, osd.small, LCD_DIM, Rect::from_min_size(pos2(x, y), vec2(ww, sh * 2.2)), Align::Near, Align::Near, true);
            y += sh * 2.2;
        }
        if !job.error.is_empty() {
            let eb = Rect::from_min_max(pos2(x, y), pos2(x + ww, card.bottom() - 4.0));
            let pts = round_pts(eb, 3.0);
            o.fill_poly(&pts, argb(190, 70, 14, 18));
            o.stroke_poly(&pts, 0.7, rgba(200, LCD_RED));
            o.write(&format!("First error\n{}", job.error.join("\n")), osd.small, LCD_INK, Rect::from_min_size(pos2(eb.left() + 4.0, eb.top() + 2.0), vec2(eb.width() - 8.0, eb.height() - 4.0)), Align::Near, Align::Near, true);
        }
        return;
    }
    let lines: Vec<&str> = job.lines.iter().rev().map(|l| l.trim()).filter(|l| !l.is_empty()).take(12).collect();
    let status = match job.fetch_wait { Some((what, secs)) => format!("Waiting {}: {what}", clock(secs)), None => lines.first().copied().unwrap_or("Starting...").to_owned() };
    o.write(&status, osd.row, LCD_INK, Rect::from_min_size(pos2(x, y), vec2(ww, osd.row.height() + 2.0)), Align::Near, Align::Near, false);
    y += osd.row.height() + 2.0;
    if job.fetch_wait.is_some() {
        o.write("git stops by itself if nothing arrives for 30 s; cancelling keeps the last fetched version.", osd.small, LCD_DIM, Rect::from_min_size(pos2(x, y), vec2(ww, sh * 2.2)), Align::Near, Align::Near, true);
        y += sh * 2.2;
    }
    for (i, l) in lines.iter().enumerate().skip(1) {
        if y + sh > card.bottom() - 3.0 { break; }
        o.write(l, osd.small, rgba((255 - i as i32 * 22).max(90) as u8, LCD_DIM), Rect::from_min_size(pos2(x, y), vec2(ww, sh + 1.0)), Align::Near, Align::Near, false);
        y += sh;
    }
}

/// `LcdSplash`: the logo, the model, the start check's step with three pulsing dots.
fn lcd_splash(o: &Ctx, osd: &Osd, w: f32, h: f32, t: f32, stage: &str, since: f32) {
    if since >= 3.0 { o.shadow_text(&clock(since), osd.small, LCD_DIM, 5.0, 4.0); }
    let lsw = o.measure("SKATE", osd.logo);
    let lsh = osd.logo.height();
    let (lx, ly) = ((w - lsw) / 2.0, h * 0.16);
    o.write("SKATE", osd.logo, LCD_INK, Rect::from_min_size(pos2(lx, ly), vec2(lsw + 4.0, lsh)), Align::Near, Align::Near, false);
    let hd = Rect::from_min_size(pos2(lx + lsw * 1.06, ly + lsh * 0.22), vec2(osd.row.height() * 1.4, osd.row.height()));
    o.gloss(hd, 3.0, Color32::from_rgb(88, 152, 242), Color32::from_rgb(26, 74, 168));
    o.write("HD", osd.row, Color32::WHITE, hd, Align::Center, Align::Center, false);
    o.write("SK8-1000  HIGH DEFINITION", osd.tiny, LCD_DIM, Rect::from_min_size(pos2(0.0, ly + lsh * 0.95), vec2(w, osd.tiny.height() + 2.0)), Align::Center, Align::Near, false);
    o.write(stage, osd.row, LCD_INK, Rect::from_min_size(pos2(0.0, h * 0.6), vec2(w, osd.row.height() + 2.0)), Align::Center, Align::Near, false);
    for i in 0..3 {
        let a = 0.35 + 0.65 * (t * 4.0 - i as f32 * 0.7).sin().max(0.0);
        o.fill_ellipse(Rect::from_min_size(pos2(w / 2.0 - 8.0 + i as f32 * 7.0, h * 0.6 + osd.row.height() + 5.0), vec2(3.4, 3.4)), rgba((255.0 * a) as u8, LCD_AMBER));
    }
}

#[cfg(test)]
mod anim_tests {
    use super::*;

    /// `--standby-test`'s checks on the clock: the fold keeps the screen lit and ends shut; the reopen stays lit and
    /// ends facing you; the power-off ends edge-on and dark.
    #[test]
    fn fold_and_reopen_keep_the_screen_lit_and_end_where_they_should() {
        for seed in 0..20 {
            let mut a = Anim::new(seed);
            a.on_at = Some(0.0);
            a.roll();
            a.fold_at = Some(10.0);
            for k in 0..=7 {
                let t = 10.0 + a.fold_time() * k as f32 / 7.0;
                assert_eq!(a.screen_level(t), 1.0, "fold frame {k} dark (seed {seed})");
            }
            assert!(a.panel_open(10.0 + a.fold_time()) < 0.01, "fold did not shut (seed {seed})");
            a.fold_at = None;
            a.resume_at = Some(20.0);
            for k in 0..=7 {
                let t = 20.0 + a.resume_time() * k as f32 / 7.0;
                assert_eq!(a.screen_level(t), 1.0, "reopen frame {k} dark (seed {seed})");
            }
            assert!((a.panel_open(20.0 + a.resume_time()) - 1.0).abs() < 0.02, "reopen did not face you (seed {seed})");
            a.resume_at = None;
            a.off_at = Some(30.0);
            assert!(a.panel_open(30.0 + OFF_TIME) < 0.01 && a.screen_level(30.0 + OFF_TIME) == 0.0, "power-off (seed {seed})");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn net_random_matches_dotnet() {
        // Values from .NET Framework's System.Random on the desktop (2026-11-10).
        let mut r = NetRandom::new(5);
        assert!((r.next_double() - 0.338_369_840_913_624_43).abs() < 1e-15);
        assert!((r.next_double() - 0.284_418_447_541_267_83).abs() < 1e-15);
        let mut z = NetRandom::new(0);
        assert_eq!((z.next(100), z.next(100)), (72, 81));
        assert_eq!(NetRandom::new(3).next(3), 0);
    }

    #[test]
    fn short_names_drop_the_lead_and_the_note() {
        assert_eq!(short_name("Play + trace all"), "Trace all");
        assert_eq!(short_name("Play with the dev test mods"), "Dev test mods");
        assert_eq!(short_name("Play: hails-additions (always the latest pushed branch)"), "Play: hails-additions");
        assert_eq!(short_name("> Upstream main (for comparing)"), "> Upstream main");
    }
}
