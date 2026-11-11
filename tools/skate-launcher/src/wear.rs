//! The panel's wear and its sticker, ported from SkateLauncher.cs (RollMarks, DrawMarks, MarkerTag, Doodle, GaffTape,
//! Creases, StickerSpec, PlaceSticker, DrawSticker, StickerShape, FilmerCut, Wobble, FitText, StickerArt; doc 41,
//! 44a 6). GDI's regions and clips are tiny-skia masks, `Widen` is a stroke outline, closed / open curves are
//! cardinal splines with GDI's tension, text is glyph outlines (ttf-parser) of the same fonts. The layer is drawn once
//! into an image in the 1240 x 760 unit space at `SCALE` pixels per unit.
use std::path::{Path, PathBuf};

use tiny_skia::{Color, FillRule, LineCap, LineJoin, Mask, Paint, PathBuilder, Pixmap, Point, Stroke, Transform};

use crate::camcorder::NetRandom;

mod stickers;

pub const SCALE: f32 = 2.0;
const PANEL: (f32, f32, f32, f32) = (20.0, 20.0, 1172.0, 720.0);
const SCREEN: (f32, f32, f32, f32) = (172.0, 52.0, 990.0, 557.0);
const W: u32 = (1240.0 * SCALE) as u32;
const H: u32 = (760.0 * SCALE) as u32;

const SCRAWL: &[&str] = &["ONE MORE TRY", "LAST TRY", "FILM IT", "SEND IT", "SESH", "DIY OR DIE", "SKATE & ANNOY", "NO COMPLY", "KICKFLIP?", "SECURITY!!", "SK8", "LINE!!", "HOMIES", "RAD", "BUST", "FILMER", "1 MORE", "SLAMS", "GNAR", "POP!", "SK8!"];
const MARKERS: &[(u8, u8, u8, u8)] = &[(232, 246, 246, 240), (220, 204, 208, 214), (225, 236, 64, 54), (225, 250, 214, 60), (225, 250, 112, 172), (225, 96, 212, 232)];
const TAG_AREAS: [(f32, f32, f32, f32); 2] = [(352.0, 630.0, 348.0, 104.0), (36.0, 640.0, 120.0, 90.0)];
const TAG_CHARS: [usize; 2] = [12, 4];
const TAG_TILT: [f32; 2] = [5.0, 8.0];
const STICKER_COUNT: i32 = stickers::FIRST + stickers::COUNT;
const STICKER_REACH: f32 = 70.0;
/// Colour / text variations per sticker design; the install's saved wear seed picks one, so it stays the same.
const VARIANTS: usize = 4;
/// Sticker wear patterns (peel side, scuffs, chips, rubbed print), rolled per install, independent of the design and
/// variation, and not user-pickable.
const WEAR_PATTERNS: i32 = 100;
/// camcorder-sticker.txt: design, zone, place x, place y, angle, wear pattern, variation. Older six-field files (the
/// last field a wear seed) still load: their seed gives the pattern and the variation.
pub type Spec = [f32; 7];

fn keep_out() -> Vec<(f32, f32, f32, f32)> {
    let g = |r: (f32, f32, f32, f32), d: f32| (r.0 - d, r.1 - d, r.2 + 2.0 * d, r.3 + 2.0 * d);
    vec![
        g(SCREEN, 15.0), (950.0, 696.0, 150.0, 30.0), (1136.0, 22.0, 30.0, 28.0),
        (34.0, 64.0, 122.0, 60.0), (30.0, 140.0, 130.0, 210.0), (34.0, 366.0, 122.0, 60.0),
        (30.0, 430.0, 130.0, 144.0), (42.0, 582.0, 106.0, 48.0), (180.0, 634.0, 104.0, 64.0),
        (176.0, 696.0, 170.0, 28.0), (710.0, 634.0, 96.0, 64.0), (820.0, 634.0, 272.0, 64.0),
    ]
}

#[derive(Clone)]
struct Mark {
    spot: usize,
    kind: i32,
    seed: i32,
    x: f32,
    y: f32,
    angle: f32,
    size: f32,
    text: String,
    ink: Color,
    placed: bool,
    fitted: bool,
    skip: bool,
}

fn rgba(a: u8, r: u8, g: u8, b: u8) -> Color {
    Color::from_rgba8(r, g, b, a)
}

fn paint(c: Color) -> Paint<'static> {
    let mut p = Paint::default();
    p.set_color(c);
    p.anti_alias = true;
    p
}

fn stroke(w: f32) -> Stroke {
    Stroke { width: w, ..Default::default() }
}

fn round_stroke(w: f32) -> Stroke {
    Stroke { width: w, line_cap: LineCap::Round, line_join: LineJoin::Round, ..Default::default() }
}

/// `Round`: a rounded rectangle path.
fn round_rect(x: f32, y: f32, w: f32, h: f32, rad: f32) -> tiny_skia::Path {
    let d = (rad * 2.0).min(w).min(h);
    let r = d / 2.0;
    let k = 0.552_284_8 * r;
    let mut pb = PathBuilder::new();
    pb.move_to(x + r, y);
    pb.line_to(x + w - r, y);
    pb.cubic_to(x + w - r + k, y, x + w, y + r - k, x + w, y + r);
    pb.line_to(x + w, y + h - r);
    pb.cubic_to(x + w, y + h - r + k, x + w - r + k, y + h, x + w - r, y + h);
    pb.line_to(x + r, y + h);
    pb.cubic_to(x + r - k, y + h, x, y + h - r + k, x, y + h - r);
    pb.line_to(x, y + r);
    pb.cubic_to(x, y + r - k, x + r - k, y, x + r, y);
    pb.close();
    pb.finish().expect("rounded rect")
}

fn rect(x: f32, y: f32, w: f32, h: f32) -> tiny_skia::Path {
    PathBuilder::from_rect(tiny_skia::Rect::from_xywh(x, y, w.max(0.01), h.max(0.01)).expect("rect"))
}

fn ellipse(x: f32, y: f32, w: f32, h: f32) -> tiny_skia::Path {
    PathBuilder::from_oval(tiny_skia::Rect::from_xywh(x, y, w.max(0.01), h.max(0.01)).expect("oval")).expect("oval")
}

fn poly(pts: &[(f32, f32)], close: bool) -> Option<tiny_skia::Path> {
    let mut pb = PathBuilder::new();
    pb.move_to(pts[0].0, pts[0].1);
    for p in &pts[1..] {
        pb.line_to(p.0, p.1);
    }
    if close {
        pb.close();
    }
    pb.finish()
}

/// GDI's cardinal spline (DrawCurve / AddClosedCurve) through `pts` with `tension`.
fn curve(pts: &[(f32, f32)], tension: f32, closed: bool) -> Option<tiny_skia::Path> {
    let n = pts.len();
    if n < 2 {
        return None;
    }
    let t = tension / 3.0;
    let at = |i: isize| -> (f32, f32) {
        if closed { pts[i.rem_euclid(n as isize) as usize] } else { pts[i.clamp(0, n as isize - 1) as usize] }
    };
    let mut pb = PathBuilder::new();
    pb.move_to(pts[0].0, pts[0].1);
    let segs = if closed { n } else { n - 1 };
    for i in 0..segs as isize {
        let (p0, p1, p2, p3) = (at(i - 1), at(i), at(i + 1), at(i + 2));
        let c1 = (p1.0 + t * (p2.0 - p0.0), p1.1 + t * (p2.1 - p0.1));
        let c2 = (p2.0 - t * (p3.0 - p1.0), p2.1 - t * (p3.1 - p1.1));
        pb.cubic_to(c1.0, c1.1, c2.0, c2.1, p2.0, p2.1);
    }
    if closed {
        pb.close();
    }
    pb.finish()
}

/// GDI's DrawArc: an arc of the ellipse in `(x, y, w, h)` from `start` degrees sweeping `sweep` degrees.
fn arc(x: f32, y: f32, w: f32, h: f32, start: f32, sweep: f32) -> Option<tiny_skia::Path> {
    let (cx, cy, rx, ry) = (x + w / 2.0, y + h / 2.0, w / 2.0, h / 2.0);
    let steps = 24;
    let pts: Vec<(f32, f32)> = (0..=steps).map(|i| {
        let a = (start + sweep * i as f32 / steps as f32).to_radians();
        // GDI measures the angle on the ellipse's circle, then scales.
        (cx + rx * a.cos(), cy + ry * a.sin())
    }).collect();
    poly(&pts, false)
}

/// A device-space mask of a path under a transform.
fn mask_of(path: &tiny_skia::Path, ts: Transform) -> Mask {
    let mut m = Mask::new(W, H).expect("mask");
    m.fill_path(path, FillRule::Winding, true, ts);
    m
}

fn and(a: &Mask, b: &Mask) -> Mask {
    let mut m = a.clone();
    for (x, y) in m.data_mut().iter_mut().zip(b.data()) {
        *x = ((*x as u16 * *y as u16) / 255) as u8;
    }
    m
}

fn and_not(a: &Mask, b: &Mask) -> Mask {
    let mut m = a.clone();
    for (x, y) in m.data_mut().iter_mut().zip(b.data()) {
        *x = ((*x as u16 * (255 - *y as u16)) / 255) as u8;
    }
    m
}

fn or(a: &mut Mask, b: &Mask) {
    for (x, y) in a.data_mut().iter_mut().zip(b.data()) {
        *x = (*x).max(*y);
    }
}

fn visible(m: &Mask, x: f32, y: f32) -> bool {
    let (px, py) = ((x * SCALE) as i64, (y * SCALE) as i64);
    if px < 0 || py < 0 || px >= W as i64 || py >= H as i64 {
        return false;
    }
    m.data()[(py as u32 * W + px as u32) as usize] >= 128
}

fn device() -> Transform {
    Transform::from_scale(SCALE, SCALE)
}

/// A unit-space path's points (curves sampled), for the "on the map" test.
fn points(path: &tiny_skia::Path) -> Vec<(f32, f32)> {
    use tiny_skia::PathSegment as S;
    let mut out = Vec::new();
    let mut last = Point::zero();
    for seg in path.segments() {
        match seg {
            S::MoveTo(p) | S::LineTo(p) => { out.push((p.x, p.y)); last = p; }
            S::QuadTo(c, p) => { for i in 1..=4 { let t = i as f32 / 4.0; let u = 1.0 - t; out.push((u * u * last.x + 2.0 * u * t * c.x + t * t * p.x, u * u * last.y + 2.0 * u * t * c.y + t * t * p.y)); } last = p; }
            S::CubicTo(c1, c2, p) => { for i in 1..=6 { let t = i as f32 / 6.0; let u = 1.0 - t; out.push((u * u * u * last.x + 3.0 * u * u * t * c1.x + 3.0 * u * t * t * c2.x + t * t * t * p.x, u * u * u * last.y + 3.0 * u * u * t * c1.y + 3.0 * u * t * t * c2.y + t * t * t * p.y)); } last = p; }
            S::Close => {}
        }
    }
    out
}

fn mark_ts(mk: &Mark) -> Transform {
    Transform::from_translate(mk.x, mk.y).pre_rotate(mk.angle)
}

/// `World`: the local outline placed at the mark, widened by a pen of `widen`.
fn world(local: &tiny_skia::Path, mk: &Mark, widen: f32) -> Option<tiny_skia::Path> {
    let p = local.clone().transform(mark_ts(mk))?;
    if widen > 0.0 { p.stroke(&stroke(widen), 1.0) } else { Some(p) }
}

/// `Inside`: the outline, widened by a 5 pen, lies wholly on the map.
fn inside(map: &Mask, local: &tiny_skia::Path, mk: &Mark) -> bool {
    world(local, mk, 5.0).is_some_and(|p| points(&p).iter().all(|(x, y)| visible(map, *x, *y)))
}

/// `Clear`: on the map and off every tag / doodle already drawn.
fn clear(map: &Mask, taken: &Mask, local: &tiny_skia::Path, mk: &Mark) -> bool {
    if !inside(map, local, mk) {
        return false;
    }
    let mut own = Mask::new(W, H).expect("mask");
    if let Some(f) = world(local, mk, 0.0) { or(&mut own, &mask_of(&f, device())); }
    if let Some(b) = world(local, mk, 5.0) { or(&mut own, &mask_of(&b, device())); }
    !own.data().iter().zip(taken.data()).any(|(a, b)| *a >= 128 && *b >= 128)
}

/// `Take`: adds the outline (and a 16 band round it) to what is taken.
fn take(taken: &mut Mask, local: &tiny_skia::Path, mk: &Mark) {
    if let Some(f) = world(local, mk, 0.0) { or(taken, &mask_of(&f, device())); }
    if let Some(b) = world(local, mk, 16.0) { or(taken, &mask_of(&b, device())); }
}

fn ring(s: f32) -> tiny_skia::Path {
    let rr = s * 1.15 + 3.0;
    ellipse(-rr, -rr, 2.0 * rr, 2.0 * rr)
}

/// A font face from the system, for outlines.
struct Face {
    data: Vec<u8>,
}

impl Face {
    fn load(files: &[&str]) -> Option<Face> {
        let dir = std::env::var_os("WINDIR").map_or_else(|| PathBuf::from(r"C:\Windows"), PathBuf::from).join("Fonts");
        let unix = ["/usr/share/fonts/truetype/dejavu", "/usr/share/fonts/TTF", "/System/Library/Fonts/Supplemental", "/Library/Fonts"];
        files.iter().find_map(|f| {
            std::iter::once(dir.join(f)).chain(unix.iter().map(|d| Path::new(d).join(f))).find_map(|p| std::fs::read(p).ok())
        }).map(|data| Face { data })
    }
    fn face(&self) -> Option<ttf_parser::Face<'_>> {
        ttf_parser::Face::parse(&self.data, 0).ok()
    }
    /// The string's glyph outlines at `size` (pixels per em), origin at the top of the line (GDI AddString).
    fn outline(&self, s: &str, size: f32) -> Option<tiny_skia::Path> {
        let face = self.face()?;
        let k = size / face.units_per_em() as f32;
        let ascent = face.tables().os2.map_or(face.ascender() as f32, |o| o.windows_ascender() as f32);
        struct B<'a> { pb: &'a mut PathBuilder, x: f32, k: f32, base: f32 }
        impl ttf_parser::OutlineBuilder for B<'_> {
            fn move_to(&mut self, x: f32, y: f32) { self.pb.move_to(self.x + x * self.k, self.base - y * self.k); }
            fn line_to(&mut self, x: f32, y: f32) { self.pb.line_to(self.x + x * self.k, self.base - y * self.k); }
            fn quad_to(&mut self, x1: f32, y1: f32, x: f32, y: f32) { self.pb.quad_to(self.x + x1 * self.k, self.base - y1 * self.k, self.x + x * self.k, self.base - y * self.k); }
            fn curve_to(&mut self, x1: f32, y1: f32, x2: f32, y2: f32, x: f32, y: f32) { self.pb.cubic_to(self.x + x1 * self.k, self.base - y1 * self.k, self.x + x2 * self.k, self.base - y2 * self.k, self.x + x * self.k, self.base - y * self.k); }
            fn close(&mut self) { self.pb.close(); }
        }
        let mut pb = PathBuilder::new();
        let mut x = 0.0;
        for ch in s.chars() {
            let Some(g) = face.glyph_index(ch) else { continue };
            let mut b = B { pb: &mut pb, x, k, base: ascent * k };
            face.outline_glyph(g, &mut b);
            x += face.glyph_hor_advance(g).unwrap_or(0) as f32 * k;
        }
        pb.finish()
    }
    /// GDI's MeasureString at `size`: advances plus the 1/6 em padding each side; the line height.
    fn measure(&self, s: &str, size: f32) -> (f32, f32) {
        let Some(face) = self.face() else { return (size * s.len() as f32 * 0.6, size * 1.2) };
        let k = size / face.units_per_em() as f32;
        let adv: f32 = s.chars().filter_map(|c| face.glyph_index(c)).map(|g| face.glyph_hor_advance(g).unwrap_or(0) as f32).sum();
        let (a, d) = face.tables().os2.map_or((face.ascender() as f32, -face.descender() as f32), |o| (o.windows_ascender() as f32, o.windows_descender() as f32));
        (adv * k + size / 3.0, (a + d) * k)
    }
}

/// A new seed for this launch (the clock's nanoseconds mixed with the process id).
pub fn launch_seed() -> i32 {
    let d = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default();
    let mixed = (d.as_nanos() as u64 ^ (u64::from(std::process::id()) << 17)).wrapping_mul(0x9E37_79B9_7F4A_7C15);
    ((mixed >> 33) & 0x7fff_ffff) as i32
}

/// `StickerSpec`: design, zone, place x, place y, angle, wear seed. One per install: rolled at the first start and
/// kept in camcorder-sticker.txt in the state folder (the C# file format); a snapshot rolls its own from its seed.
pub fn sticker_spec(state_dir: &Path, snapshot_seed: Option<i32>) -> Spec {
    let file = state_dir.join("camcorder-sticker.txt");
    if snapshot_seed.is_none() {
        if let Ok(t) = std::fs::read_to_string(&file) {
            let v: Vec<f32> = t.trim().split(' ').filter_map(|x| x.parse().ok()).collect();
            if v.len() == 7 {
                return [v[0], v[1], v[2], v[3], v[4], v[5].rem_euclid(WEAR_PATTERNS as f32), v[6]];
            }
            if v.len() == 6 {
                let seed = v[5] as i32;
                return [v[0], v[1], v[2], v[3], v[4], seed.rem_euclid(WEAR_PATTERNS) as f32, seed.rem_euclid(VARIANTS as i32) as f32];
            }
        }
    }
    let mut r = NetRandom::new(snapshot_seed.unwrap_or_else(launch_seed));
    let soft = r.next(2) == 0;
    let pool: Vec<i32> = (0..STICKER_COUNT).filter(|d| stickers::SOFT.contains(d) == soft).collect();
    let design = pool[r.next(pool.len() as i32) as usize];
    let s = [design as f32, r.next(3) as f32, r.next_double() as f32, r.next_double() as f32, (r.next_double() * 40.0 - 20.0) as f32, r.next(WEAR_PATTERNS) as f32, r.next(VARIANTS as i32) as f32];
    if snapshot_seed.is_none() {
        let _ = std::fs::create_dir_all(state_dir);
        let _ = std::fs::write(&file, s.iter().map(|x| x.to_string()).collect::<Vec<_>>().join(" "));
    }
    s
}

fn sticker_at(s: &Spec) -> (f32, f32) {
    (420.0 + s[2] * 200.0, 682.0 + s[3] * 4.0)
}

/// `SpotArea`: the bottom spot gives way to the sticker.
fn spot_area(spot: usize, s: &Spec) -> (f32, f32, f32, f32) {
    let a = TAG_AREAS[spot];
    if spot != 0 {
        return a;
    }
    let sx = sticker_at(s).0;
    let (left, right) = (sx - STICKER_REACH - a.0, a.0 + a.2 - (sx + STICKER_REACH));
    if left >= right { (a.0, a.1, left.max(20.0), a.3) } else { (sx + STICKER_REACH, a.1, right.max(20.0), a.3) }
}

/// `RollMarks`.
fn roll_marks(r: &mut NetRandom) -> Vec<Mark> {
    let mut marks = Vec::new();
    let u = |r: &mut NetRandom, lo: f64, hi: f64| (lo + r.next_double() * (hi - lo)) as f32;
    let base = Mark { spot: 0, kind: 0, seed: 0, x: 0.0, y: 0.0, angle: 0.0, size: 0.0, text: String::new(), ink: Color::BLACK, placed: false, fitted: false, skip: false };
    let ink = |r: &mut NetRandom| { let m = MARKERS[r.next(MARKERS.len() as i32) as usize]; rgba(m.0, m.1, m.2, m.3) };
    let mut free = vec![0usize, 1];
    let tags = 1 + r.next(2);
    for _ in 0..tags {
        let spot = free[r.next(free.len() as i32) as usize];
        free.retain(|s| *s != spot);
        let a = TAG_AREAS[spot];
        let x = a.0 + a.2 * u(r, 0.35, 0.65);
        let y = a.1 + a.3 * u(r, 0.4, 0.6);
        let angle = TAG_TILT[spot] * u(r, -1.0, 1.0);
        let fit: Vec<&str> = SCRAWL.iter().copied().filter(|w| w.len() <= TAG_CHARS[spot]).collect();
        let text = fit[r.next(fit.len() as i32) as usize].to_owned();
        let ink = ink(r);
        let seed = r.next_raw();
        marks.push(Mark { spot, kind: 0, x, y, angle, text, ink, seed, ..base.clone() });
    }
    let doodles = 1 + r.next(2);
    for _ in 0..doodles {
        let spot = r.next(2) as usize;
        let a = TAG_AREAS[spot];
        let x = a.0 + a.2 * u(r, 0.1, 0.9);
        let y = a.1 + a.3 * u(r, 0.3, 0.7);
        let angle = u(r, -20.0, 20.0);
        let size = (a.3 * 0.7).min(46.0) * u(r, 0.7, 1.0);
        let ink = ink(r);
        let seed = r.next(5);
        marks.push(Mark { spot, kind: 1, x, y, angle, size, ink, seed, ..base.clone() });
    }
    let (px, py, pw, ph) = PANEL;
    let (_, sy, _, sh) = SCREEN;
    let scratches = 3 + r.next(4);
    for _ in 0..scratches {
        let x = u(r, (px + 30.0) as f64, (px + pw - 40.0) as f64);
        let y = if r.next(2) == 0 { u(r, (py + 6.0) as f64, (sy - 14.0) as f64) } else { u(r, (sy + sh + 16.0) as f64, (py + ph - 10.0) as f64) };
        let angle = u(r, -30.0, 30.0);
        let size = u(r, 30.0, 90.0);
        let seed = r.next_raw();
        marks.push(Mark { kind: 2, x, y, angle, size, seed, ..base.clone() });
    }
    let edges = 1 + r.next(2);
    for _ in 0..edges {
        let side = r.next(3);
        let x = if side == 2 { px + 3.0 } else { u(r, (px + 60.0) as f64, (px + pw - 340.0) as f64) };
        let y = if side == 0 { py + 3.0 } else if side == 1 { py + ph - 3.0 } else { u(r, (py + 60.0) as f64, (py + ph - 340.0) as f64) };
        let angle = if side == 2 { 90.0 } else { 0.0 };
        let size = u(r, 120.0, 300.0);
        let seed = r.next_raw();
        marks.push(Mark { kind: 3, x, y, angle, size, seed, ..base.clone() });
    }
    if r.next(2) == 0 {
        let left = r.next(2) == 0;
        let x = if left { px + 26.0 } else { px + pw - 26.0 };
        let y = py + ph - 26.0;
        let angle = (if left { 45.0 } else { -45.0 }) + u(r, -8.0, 8.0);
        let size = u(r, 80.0, 110.0);
        let seed = r.next_raw();
        marks.push(Mark { kind: 4, x, y, angle, size, seed, ..base.clone() });
    }
    marks
}

/// The wear map (the panel minus the keep-outs) and the tag map (also minus the sticker's footprint).
fn maps(s: &Spec) -> (Mask, Mask) {
    let (px, py, pw, ph) = PANEL;
    let mut wear = mask_of(&round_rect(px, py, pw, ph, 34.0), device());
    let data = wear.data_mut();
    for (x, y, w, h) in keep_out() {
        let (x0, y0) = (((x * SCALE).floor().max(0.0)) as u32, ((y * SCALE).floor().max(0.0)) as u32);
        let (x1, y1) = (((x + w) * SCALE).ceil().min(W as f32) as u32, ((y + h) * SCALE).ceil().min(H as f32) as u32);
        for yy in y0..y1 {
            for xx in x0..x1 {
                data[(yy * W + xx) as usize] = 0;
            }
        }
    }
    let (sx, sy) = sticker_at(s);
    let foot = mask_of(&ellipse(sx - STICKER_REACH, sy - STICKER_REACH, 2.0 * STICKER_REACH, 2.0 * STICKER_REACH), device());
    let tag = and_not(&wear, &foot);
    (wear, tag)
}

/// `FindRoom`: the doodle's rolled place, else the nearest free place in either spot.
fn find_room(map: &Mask, taken: &Mask, mk: &mut Mark, s: f32, spec: &Spec) -> bool {
    let (x0, y0) = (mk.x, mk.y);
    let mut spots = Vec::new();
    for spot in 0..TAG_AREAS.len() {
        let a = spot_area(spot, spec);
        for ix in 1..=9 {
            for iy in (3..=7).step_by(2) {
                spots.push((a.0 + a.2 * ix as f32 / 10.0, a.1 + a.3 * iy as f32 / 10.0));
            }
        }
    }
    spots.sort_by(|p, q| ((p.0 - x0).powi(2) + (p.1 - y0).powi(2)).partial_cmp(&((q.0 - x0).powi(2) + (q.1 - y0).powi(2))).unwrap_or(std::cmp::Ordering::Equal));
    spots.insert(0, (x0, y0));
    let ring = ring(s);
    for p in spots {
        mk.x = p.0;
        mk.y = p.1;
        if clear(map, taken, &ring, mk) {
            return true;
        }
    }
    mk.x = x0;
    mk.y = y0;
    false
}

/// `Wobble`: a wobbly hand-drawn line (GDI DrawCurve, tension 0.5).
fn wobble(px: &mut Pixmap, ts: Transform, clip: Option<&Mask>, pen: &Stroke, c: Color, r: &mut NetRandom, pts: &[(f32, f32)]) {
    let w: Vec<(f32, f32)> = pts.iter().map(|p| (p.0 + (r.next_double() * 2.4 - 1.2) as f32, p.1 + (r.next_double() * 2.4 - 1.2) as f32)).collect();
    if let Some(path) = curve(&w, 0.5, false) {
        px.stroke_path(&path, &paint(c), pen, ts, clip);
    }
}

/// The wear and sticker layer for the panel.
/// `stickers`: the state folder's stickers folder; its PNGs (the game's own stickers once the asset export finds
/// them, or the player's own) replace the drawn designs (SkateLauncher.cs GameSticker).
pub fn render(spec: &Spec, marks_seed: i32, stickers: Option<&Path>) -> Pixmap {
    let mut px = Pixmap::new(W, H).expect("pixmap");
    let (wear_map, tag_map) = maps(spec);
    let mut marks = roll_marks(&mut NetRandom::new(marks_seed));
    let hand = Face::load(&["Inkfree.ttf", "segoepr.ttf", "comic.ttf", "DejaVuSans.ttf"]);
    let mut taken = Mask::new(W, H).expect("mask");
    let mut ink = Pixmap::new(W, H).expect("pixmap");
    for mk in marks.iter_mut() {
        if mk.skip { continue; }
        if !mk.placed && (mk.kind == 0 || mk.kind == 1) {
            let a = spot_area(mk.spot, spec);
            let t = TAG_AREAS[mk.spot];
            mk.x = a.0 + a.2 / 2.0 + (mk.x - (t.0 + t.2 / 2.0)) * a.2 / t.2;
            mk.placed = true;
        }
        let mut rnd = NetRandom::new(mk.seed);
        let ts = device().pre_concat(mark_ts(mk));
        match mk.kind {
            0 => if let Some(h) = &hand { marker_tag(&mut ink, &wear_map, &tag_map, &mut taken, mk, &mut rnd, h, spec) },
            1 => doodle(&mut ink, &wear_map, &tag_map, &mut taken, mk, &mut rnd, spec),
            2 => scratches(&mut px, mk, &mut rnd, ts, &wear_map),
            3 => rubbed_edge(&mut px, mk, &mut rnd, ts, &wear_map),
            _ => gaff_tape(&mut px, mk, &mut rnd, ts, &wear_map),
        }
    }
    ink_on_plastic(&mut ink, marks_seed);
    px.draw_pixmap(0, 0, ink.as_ref(), &tiny_skia::PixmapPaint::default(), Transform::identity(), None);
    place_sticker(&mut px, spec, stickers);
    surface(&mut px, marks_seed);
    px
}

/// A cluster of scratches: each a gouge (dark groove, lit far lip) that tapers in and out along a slight curve, with a
/// haze of fine hairlines around them.
fn scratches(px: &mut Pixmap, mk: &Mark, rnd: &mut NetRandom, ts: Transform, wear: &Mask) {
    let n = 5 + rnd.next(8);
    let line = |px: &mut Pixmap, a: (f32, f32), b: (f32, f32), w: f32, c: Color| {
        if let Some(p) = poly(&[a, b], false) { px.stroke_path(&p, &paint(c), &round_stroke(w), ts, Some(wear)); }
    };
    for _ in 0..n {
        let x0 = (rnd.next_double() * mk.size as f64 - mk.size as f64 / 2.0) as f32;
        let y0 = (rnd.next_double() * 18.0 - 9.0) as f32;
        let len = (rnd.next_double() * mk.size as f64 * 0.6 + 8.0) as f32;
        let dy = (rnd.next_double() * 6.0 - 3.0) as f32;
        let bow = (rnd.next_double() * 3.0 - 1.5) as f32;
        let deep = rnd.next_double() as f32;
        let steps = 10;
        let at = |t: f32| (x0 + len * t, y0 + dy * t + bow * 4.0 * t * (1.0 - t));
        for k in 0..steps {
            let (t0, t1) = (k as f32 / steps as f32, (k + 1) as f32 / steps as f32);
            let taper = (std::f32::consts::PI * (t0 + t1) / 2.0).sin();
            let (p0, p1) = (at(t0), at(t1));
            let w = 0.5 + taper * (0.5 + deep * 1.1);
            line(px, (p0.0, p0.1 + 0.8), (p1.0, p1.1 + 0.8), w, rgba((70.0 * taper) as u8, 0, 0, 0));
            line(px, p0, p1, w * 0.7, rgba((40.0 + 75.0 * taper * (0.5 + deep)) as u8, 196, 200, 208));
        }
    }
    for _ in 0..18 {
        let x0 = (rnd.next_double() * mk.size as f64 * 1.2 - mk.size as f64 * 0.6) as f32;
        let y0 = (rnd.next_double() * 26.0 - 13.0) as f32;
        let len = (rnd.next_double() * 14.0 + 3.0) as f32;
        let dy = (rnd.next_double() * 3.0 - 1.5) as f32;
        line(px, (x0, y0), (x0 + len, y0 + dy), 0.5, rgba(26 + rnd.next(22) as u8, 210, 214, 220));
    }
}

/// Paint rubbed off along the frame's edge: a ragged band of bare, lighter plastic, thickest in the middle, flecked
/// and scuffed, fading out at both ends.
fn rubbed_edge(px: &mut Pixmap, mk: &Mark, rnd: &mut NetRandom, ts: Transform, wear: &Mask) {
    for _ in 0..140 {
        let t = rnd.next_double() as f32;
        let x0 = t * mk.size;
        let reach = (std::f32::consts::PI * t).sin();
        let off = ((rnd.next_double() + rnd.next_double()) as f32 - 1.0) * 5.0 * reach;
        let len = (rnd.next_double() * 9.0 + 1.5) as f32 * (0.4 + reach);
        let width = (rnd.next_double() * 1.4 + 0.5) as f32 * (0.5 + reach);
        let a = (30.0 + rnd.next_double() * 90.0) as f32 * (0.3 + 0.7 * reach) * (1.0 - off.abs() / 7.0).max(0.2);
        let end = off + (rnd.next_double() * 1.2 - 0.6) as f32;
        if let Some(p) = poly(&[(x0, off), (x0 + len, end)], false) {
            px.stroke_path(&p, &paint(rgba(a as u8, 132, 135, 144)), &round_stroke(width), ts, Some(wear));
        }
    }
    for _ in 0..40 {
        let t = rnd.next_double() as f32;
        let reach = (std::f32::consts::PI * t).sin();
        let (x, y) = (t * mk.size, ((rnd.next_double() * 2.0 - 1.0) * 6.0) as f32 * reach);
        let r = (rnd.next_double() * 1.2 + 0.4) as f32;
        px.fill_path(&ellipse(x - r, y - r, 2.0 * r, 2.0 * r), &paint(rgba((60.0 + 80.0 * reach) as u8, 168, 171, 180)), FillRule::Winding, ts, Some(wear));
    }
}

/// A sticker that has lived on a camera: paper grain, the print rubbed thin in patches (thumbs, pockets), and dirt
/// worked in along its edge.
fn sticker_weathering(px: &mut Pixmap, kept: &Mask, seed: i32) {
    let seed = seed as u32 ^ 0x5717;
    let m = kept.data();
    let (mut x0, mut y0, mut x1, mut y1) = (W, H, 0, 0);
    for y in 0..H {
        for x in 0..W {
            if m[(y * W + x) as usize] > 0 { x0 = x0.min(x); y0 = y0.min(y); x1 = x1.max(x); y1 = y1.max(y); }
        }
    }
    if x1 < x0 { return; }
    const R: i32 = 9;
    let d = px.data_mut();
    for y in y0..=y1 {
        for x in x0..=x1 {
            let j = (y * W + x) as usize;
            if m[j] < 250 { continue; }
            let mut edge = R as f32;
            for oy in -R..=R {
                for ox in -R..=R {
                    let (xx, yy) = (x as i32 + ox, y as i32 + oy);
                    let out = xx < 0 || yy < 0 || xx >= W as i32 || yy >= H as i32 || m[(yy as u32 * W + xx as u32) as usize] < 128;
                    if out { edge = edge.min(((ox * ox + oy * oy) as f32).sqrt()); }
                }
            }
            let (u, v) = (x as f32 / SCALE, y as f32 / SCALE);
            let i = j * 4;
            let a = d[i + 3] as f32 / 255.0;
            let rub = noise(u / 11.0, v / 11.0, seed) * 0.8 + noise(u / 2.5, v / 2.5, seed ^ 9) * 0.2;
            let fade = ((rub - 0.64) * 1.6).clamp(0.0, 0.3);
            let paper = [226.0, 220.0, 204.0];
            for c in 0..3 { d[i + c] = (d[i + c] as f32 * (1.0 - fade) + paper[c] * a * fade) as u8; }
            let grain = 0.94 + 0.12 * noise(u * 1.6, v * 1.6, seed ^ 0x33);
            let dirt = (1.0 - edge / R as f32).max(0.0).powf(1.6) * (0.35 + 0.65 * noise(u / 3.0, v / 3.0, seed ^ 0x71)) * 0.5;
            for c in 0..3 { d[i + c] = ((d[i + c] as f32 * grain).min(255.0 * a) * (1.0 - dirt) + 28.0 * a * dirt) as u8; }
        }
    }
}

/// Smooth value noise in [0, 1].
fn noise(x: f32, y: f32, seed: u32) -> f32 {
    let h = |ix: i32, iy: i32| {
        let mut v = (ix as u32).wrapping_mul(0x9E37_79B1) ^ (iy as u32).wrapping_mul(0x85EB_CA77) ^ seed.wrapping_mul(0xC2B2_AE3D);
        v ^= v >> 15;
        v = v.wrapping_mul(0x2C1B_3C6D);
        v ^= v >> 12;
        (v & 0xFFFF) as f32 / 65535.0
    };
    let (fx, fy) = (x.floor(), y.floor());
    let (tx, ty) = (x - fx, y - fy);
    let (tx, ty) = (tx * tx * (3.0 - 2.0 * tx), ty * ty * (3.0 - 2.0 * ty));
    let (ix, iy) = (fx as i32, fy as i32);
    let top = h(ix, iy) + (h(ix + 1, iy) - h(ix, iy)) * tx;
    let bot = h(ix, iy + 1) + (h(ix + 1, iy + 1) - h(ix, iy + 1)) * tx;
    top + (bot - top) * ty
}

/// Paint marker on textured plastic: streaky coverage along the stroke, pores where the grain shows through.
fn ink_on_plastic(ink: &mut Pixmap, seed: i32) {
    let seed = seed as u32;
    let d = ink.data_mut();
    for y in 0..H {
        for x in 0..W {
            let i = ((y * W + x) * 4) as usize;
            if d[i + 3] == 0 { continue; }
            let (u, v) = (x as f32 / SCALE, y as f32 / SCALE);
            let streak = noise(u / 9.0, v / 1.4, seed ^ 0x51);
            let pore = noise(u / 1.3, v / 1.3, seed ^ 0xA7);
            let mut k = 0.62 + 0.38 * streak;
            if pore > 0.7 { k *= 1.0 - (pore - 0.7) * 2.2; }
            for c in 0..4 { d[i + c] = (d[i + c] as f32 * k.clamp(0.0, 1.0)) as u8; }
        }
    }
}

/// The plastic's own surface over everything on it: the panel's top-down light (its 76 to 32 gradient) on the marks
/// and the sticker, fine grain, dirt gathered along the frame's lip and the glass gasket, and a faint sheen.
fn surface(px: &mut Pixmap, seed: i32) {
    let seed = seed as u32;
    let (ppx, ppy, ppw, pph) = PANEL;
    let panel = mask_of(&round_rect(ppx, ppy, ppw, pph, 34.0), device());
    let (sx, sy, sw, sh) = SCREEN;
    let glass = mask_of(&round_rect(sx - 13.0, sy - 13.0, sw + 26.0, sh + 26.0, 10.0), device());
    let (pm, gm) = (panel.data(), glass.data());
    let d = px.data_mut();
    let rr = |u: f32, v: f32, x: f32, y: f32, w: f32, h: f32, r: f32| {
        let (cx, cy) = (x + w / 2.0, y + h / 2.0);
        let (qx, qy) = ((u - cx).abs() - (w / 2.0 - r), (v - cy).abs() - (h / 2.0 - r));
        (qx.max(0.0).hypot(qy.max(0.0)) + qx.max(qy).min(0.0) - r).abs()
    };
    for y in 0..H {
        for x in 0..W {
            let j = (y * W + x) as usize;
            let cover = pm[j] as f32 / 255.0 * (1.0 - gm[j] as f32 / 255.0);
            if cover <= 0.0 { continue; }
            let i = j * 4;
            let (u, v) = (x as f32 / SCALE, y as f32 / SCALE);
            if d[i + 3] > 0 {
                let light = 0.74 + 0.26 * (1.0 - ((v - ppy) / pph).clamp(0.0, 1.0));
                for c in 0..3 { d[i + c] = (d[i + c] as f32 * light) as u8; }
            }
            let edge = rr(u, v, ppx + 6.0, ppy + 6.0, ppw - 12.0, pph - 12.0, 28.0).min(rr(u, v, sx - 13.0, sy - 13.0, sw + 26.0, sh + 26.0, 10.0));
            let blotch = noise(u / 14.0, v / 14.0, seed ^ 0x3C);
            let grime = (1.0 - edge / 9.0).max(0.0) * (0.35 + 0.65 * blotch) * 0.32 + blotch * blotch * 0.05;
            let grain = noise(u * 1.1, v * 1.1, seed ^ 0x77) - 0.5;
            let sheen = (1.0 - ((u * 0.55 + v) - 330.0).abs() / 260.0).max(0.0) * 0.045;
            let mut over = |a: f32, r: f32, g: f32, b: f32| {
                let a = (a * cover).clamp(0.0, 1.0);
                for (c, s) in [(0, r), (1, g), (2, b)] { d[i + c] = (s * a + d[i + c] as f32 * (1.0 - a)) as u8; }
                d[i + 3] = (a * 255.0 + d[i + 3] as f32 * (1.0 - a)) as u8;
            };
            over(grime, 8.0, 8.0, 10.0);
            if grain < 0.0 { over(-grain * 0.07, 0.0, 0.0, 0.0) } else { over(grain * 0.05, 255.0, 255.0, 255.0) }
            over(sheen, 255.0, 255.0, 255.0);
        }
    }
}

/// `MarkerTag`: the word in the hand font, sized to its spot, shrunk until it fits, paint worn through in places.
#[allow(clippy::too_many_arguments)]
fn marker_tag(px: &mut Pixmap, wear: &Mask, map: &Mask, taken: &mut Mask, mk: &Mark, rnd: &mut NetRandom, hand: &Face, spec: &Spec) {
    let area = spot_area(mk.spot, spec);
    let Some(path) = hand.outline(&mk.text, area.3 * 0.7) else { return };
    let b = path.bounds();
    let k = (area.2 * 0.85 / b.width().max(1.0)).min(1.0);
    let mut path = match path.transform(Transform::from_scale(k, k).pre_translate(-b.x() - b.width() / 2.0, -b.y() - b.height() / 2.0)) { Some(p) => p, None => return };
    for _ in 0..12 {
        if clear(map, taken, &path, mk) { break; }
        match path.clone().transform(Transform::from_scale(0.9, 0.9)) { Some(p) => path = p, None => break }
    }
    take(taken, &path, mk);
    let ts = device().pre_concat(mark_ts(mk));
    px.fill_path(&path, &paint(mk.ink), FillRule::Winding, ts, Some(wear));
    let edge = Color::from_rgba8((mk.ink.red() * 255.0) as u8, (mk.ink.green() * 255.0) as u8, (mk.ink.blue() * 255.0) as u8, 120);
    px.stroke_path(&path, &paint(edge), &stroke(1.2), ts, Some(wear));
    let inside_mask = and(&mask_of(&path, ts), wear);
    let nb = path.bounds();
    for _ in 0..60 {
        let (x, y) = (nb.x() + rnd.next_double() as f32 * nb.width(), nb.y() + rnd.next_double() as f32 * nb.height());
        let (w, h) = (1.5 + rnd.next_double() as f32 * 3.0, 1.0 + rnd.next_double() as f32 * 2.0);
        px.fill_path(&ellipse(x, y, w, h), &paint(rgba(170, 50, 52, 58)), FillRule::Winding, ts, Some(&inside_mask));
    }
    for _ in 0..5 {
        let y = nb.y() + rnd.next_double() as f32 * nb.height();
        let y2 = y + (rnd.next_double() * 4.0 - 2.0) as f32;
        if let Some(p) = poly(&[(nb.x(), y), (nb.right(), y2)], false) { px.stroke_path(&p, &paint(rgba(90, 50, 52, 58)), &stroke(1.6), ts, Some(&inside_mask)); }
    }
}

/// `Doodle`: crown, star, arrow, X or smiley, shrunk until it fits (pen width included).
fn doodle(px: &mut Pixmap, wear: &Mask, map: &Mask, taken: &mut Mask, mk: &mut Mark, rnd: &mut NetRandom, spec: &Spec) {
    let mut s = mk.size / 2.0;
    if !mk.fitted {
        mk.fitted = true;
        if !find_room(map, taken, mk, s * 0.7, spec) { mk.skip = true; return; }
    }
    for _ in 0..12 {
        if clear(map, taken, &ring(s), mk) { break; }
        s *= 0.88;
    }
    take(taken, &ring(s), mk);
    let ts = device().pre_concat(mark_ts(mk));
    let pen = round_stroke((s * 0.12).max(2.4));
    let c = mk.ink;
    let clip = Some(wear);
    match mk.seed {
        0 => wobble(px, ts, clip, &pen, c, rnd, &[(-s, s * 0.6), (-s, -s * 0.5), (-s * 0.5, s * 0.05), (0.0, -s * 0.8), (s * 0.5, s * 0.05), (s, -s * 0.5), (s, s * 0.6), (-s, s * 0.6)]),
        1 => {
            let pts: Vec<(f32, f32)> = (0..=10).map(|i| { let a = -std::f32::consts::FRAC_PI_2 + i as f32 * std::f32::consts::PI / 5.0; let rr = if i % 2 == 0 { s } else { s * 0.42 }; (a.cos() * rr, a.sin() * rr) }).collect();
            wobble(px, ts, clip, &pen, c, rnd, &pts);
        }
        2 => {
            wobble(px, ts, clip, &pen, c, rnd, &[(-s, s * 0.2), (-s * 0.2, -s * 0.1), (s, 0.0)]);
            wobble(px, ts, clip, &pen, c, rnd, &[(s * 0.55, -s * 0.45), (s, 0.0), (s * 0.5, s * 0.4)]);
        }
        3 => {
            wobble(px, ts, clip, &pen, c, rnd, &[(-s * 0.7, -s * 0.7), (0.0, 0.0), (s * 0.7, s * 0.7)]);
            wobble(px, ts, clip, &pen, c, rnd, &[(s * 0.7, -s * 0.7), (0.0, 0.0), (-s * 0.7, s * 0.7)]);
        }
        _ => {
            px.stroke_path(&ellipse(-s, -s, 2.0 * s, 2.0 * s), &paint(c), &pen, ts, clip);
            if let Some(a) = arc(-s * 0.55, -s * 0.3, s * 1.1, s * 0.95, 20.0, 140.0) { px.stroke_path(&a, &paint(c), &pen, ts, clip); }
            if let Some(l) = poly(&[(-s * 0.35, -s * 0.35), (-s * 0.35, -s * 0.15)], false) { px.stroke_path(&l, &paint(c), &pen, ts, clip); }
            if let Some(l) = poly(&[(s * 0.35, -s * 0.35), (s * 0.35, -s * 0.15)], false) { px.stroke_path(&l, &paint(c), &pen, ts, clip); }
        }
    }
}

/// `GaffTape`: a strip of matte tape across a corner, torn at both ends.
fn gaff_tape(px: &mut Pixmap, mk: &Mark, rnd: &mut NetRandom, ts: Transform, wear: &Mask) {
    let (w, h) = (mk.size, 46.0f32);
    let mut pts = Vec::new();
    for i in 0..=8 { pts.push((-w / 2.0 + (rnd.next_double() * 5.0 - 2.5) as f32, -h / 2.0 + i as f32 * h / 8.0)); }
    for i in (0..=8).rev() { pts.push((w / 2.0 + (rnd.next_double() * 5.0 - 2.5) as f32, -h / 2.0 + i as f32 * h / 8.0)); }
    let panel = wear.clone();
    let shadow: Vec<(f32, f32)> = pts.iter().map(|p| (p.0 + 2.0, p.1 + 2.0)).collect();
    if let Some(p) = poly(&shadow, true) { px.fill_path(&p, &paint(rgba(90, 0, 0, 0)), FillRule::Winding, ts, Some(&panel)); }
    if let Some(p) = poly(&pts, true) {
        let shader = tiny_skia::LinearGradient::new(Point::from_xy(0.0, -h / 2.0 - 1.0), Point::from_xy(0.0, h / 2.0 + 1.0),
            vec![tiny_skia::GradientStop::new(0.0, Color::from_rgba8(78, 78, 82, 255)), tiny_skia::GradientStop::new(1.0, Color::from_rgba8(34, 34, 38, 255))], tiny_skia::SpreadMode::Pad, Transform::identity());
        let mut paint = Paint::default();
        paint.anti_alias = true;
        if let Some(sh) = shader { paint.shader = sh; }
        px.fill_path(&p, &paint, FillRule::Winding, ts, Some(&panel));
    }
    for _ in 0..4 {
        let x = (rnd.next_double() * w as f64 - w as f64 / 2.0) as f32;
        let x2 = x + (rnd.next_double() * 10.0 - 5.0) as f32;
        if let Some(l) = poly(&[(x, -h / 2.0 + 4.0), (x2, h / 2.0 - 4.0)], false) { px.stroke_path(&l, &self::paint(rgba(50, 255, 255, 255)), &stroke(1.0), ts, Some(&panel)); }
    }
    if let Some(l) = poly(&[(-w / 2.0 + 4.0, -h / 2.0 + 8.0), (w / 2.0 - 4.0, -h / 2.0 + 8.0)], false) { px.stroke_path(&l, &self::paint(rgba(40, 255, 255, 255)), &stroke(2.0), ts, Some(&panel)); }
}

/// `Ridges`: the gasket edge and the bezel's inner lip under the sticker.
fn ridges() -> [[(f32, f32); 2]; 2] {
    let (px, _, pw, _) = PANEL;
    let bottom = SCREEN.1 + SCREEN.3;
    [[(px, bottom + 13.0), (px + pw, bottom + 13.0)], [(px, PANEL.1 + PANEL.3 - 10.0), (px + pw, PANEL.1 + PANEL.3 - 10.0)]]
}

fn creases(px: &mut Pixmap, ts: Transform, clip: Option<&Mask>, ridges: &[[(f32, f32); 2]]) {
    for seg in ridges {
        let (ex, ey) = (seg[1].0 - seg[0].0, seg[1].1 - seg[0].1);
        let el = (ex * ex + ey * ey).sqrt();
        if el < 1.0 { continue; }
        let (ox, oy) = (-ey / el * 1.8, ex / el * 1.8);
        if let Some(l) = poly(&[seg[0], seg[1]], false) { px.stroke_path(&l, &paint(rgba(100, 0, 0, 0)), &stroke(2.4), ts, clip); }
        if let Some(l) = poly(&[(seg[0].0 + ox, seg[0].1 + oy), (seg[1].0 + ox, seg[1].1 + oy)], false) { px.stroke_path(&l, &paint(rgba(90, 255, 255, 255)), &stroke(1.5), ts, clip); }
    }
}

/// `PlaceSticker`: the sticker on the bezel's bottom strip, rotated and scaled, the surface's ridges in its own space.
/// The stickers folder's PNGs, in name order.
fn game_stickers(dir: Option<&Path>) -> Vec<PathBuf> {
    let Some(dir) = dir else { return Vec::new() };
    let mut list: Vec<PathBuf> = std::fs::read_dir(dir).map(|r| r.flatten().map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x.eq_ignore_ascii_case("png"))).collect()).unwrap_or_default();
    list.sort();
    list
}

fn png_pixmap(path: &Path) -> Option<Pixmap> {
    let img = image::open(path).ok()?.to_rgba8();
    let (w, h) = img.dimensions();
    let mut px = Pixmap::new(w, h)?;
    for (dst, src) in px.pixels_mut().iter_mut().zip(img.pixels()) {
        let c = tiny_skia::ColorU8::from_rgba(src[0], src[1], src[2], src[3]);
        *dst = c.premultiply();
    }
    Some(px)
}

fn place_sticker(px: &mut Pixmap, s: &Spec, stickers: Option<&Path>) {
    let (cx, cy) = sticker_at(s);
    let scale = 0.68;
    let ra = -s[4].to_radians();
    let (rc, rs) = (ra.cos(), ra.sin());
    let local: Vec<[(f32, f32); 2]> = ridges().iter().map(|seg| {
        let f = |q: (f32, f32)| { let (dx, dy) = (q.0 - cx, q.1 - cy); ((dx * rc - dy * rs) / scale, (dx * rs + dy * rc) / scale) };
        [f(seg[0]), f(seg[1])]
    }).collect();
    let ts = device().pre_translate(cx, cy).pre_rotate(s[4]).pre_scale(scale, scale);
    let list = game_stickers(stickers);
    if let Some(img) = (!list.is_empty()).then(|| png_pixmap(&list[s[5] as usize % list.len()])).flatten() {
        // Scaled so its longer side is 160 units, centred, the surface's ridges creased into it.
        let k = 160.0 / img.width().max(img.height()) as f32;
        let (w, h) = (img.width() as f32 * k, img.height() as f32 * k);
        let at = ts.pre_translate(-w / 2.0, -h / 2.0).pre_scale(k, k);
        px.draw_pixmap(0, 0, img.as_ref(), &tiny_skia::PixmapPaint { quality: tiny_skia::FilterQuality::Bicubic, ..Default::default() }, at, None);
        let clip = mask_of(&rect(-w / 2.0, -h / 2.0, w, h), ts);
        creases(px, ts, Some(&clip), &local);
        return;
    }
    draw_sticker(px, s[0] as i32, s[6] as usize, s[5] as i32, &local, ts, false);
}

/// `--sticker-sheet <png> [design]`: every sticker design (or one, larger) in every variation, as previews.
pub fn sticker_sheet(out: &Path, only: Option<i32>) -> Result<(), String> {
    let mut px = Pixmap::new(W, H).ok_or("pixmap")?;
    px.fill(Color::from_rgba8(46, 47, 53, 255));
    let designs: Vec<usize> = match only { Some(d) => vec![d as usize], None => (0..STICKER_COUNT as usize).collect() };
    let per_row = if only.is_some() { 1 } else { 4 };
    let rows = designs.len().div_ceil(per_row);
    let scale = if only.is_some() { 1.5 } else { 0.42 };
    for (i, &d) in designs.iter().enumerate() {
        for v in 0..VARIANTS {
            let (col, row) = ((i % per_row) * VARIANTS + v, i / per_row);
            let (cx, cy) = (1240.0 / (per_row * VARIANTS) as f32 * (col as f32 + 0.5), 760.0 / rows as f32 * (row as f32 + 0.5));
            let ts = device().pre_translate(cx, cy).pre_scale(scale, scale);
            draw_sticker(&mut px, d as i32, v, 0, &[], ts, true);
        }
    }
    let mut img = image::RgbaImage::new(W, H);
    for (p, q) in px.pixels().iter().zip(img.pixels_mut()) {
        let c = p.demultiply();
        *q = image::Rgba([c.red(), c.green(), c.blue(), c.alpha()]);
    }
    img.save(out).map_err(|e| e.to_string())
}

/// `StickerShape`.
fn sticker_shape(design: i32) -> Option<tiny_skia::Path> {
    match design {
        0 => curve(&filmer_cut(), 0.5, true),
        1 => {
            let mut pb = PathBuilder::new();
            let a = arc(-80.0, -56.0, 160.0, 70.0, 180.0, 180.0)?;
            let pts = points(&a);
            pb.move_to(pts[0].0, pts[0].1);
            for p in &pts[1..] { pb.line_to(p.0, p.1); }
            pb.line_to(80.0, -21.0);
            pb.line_to(80.0, 40.0);
            pb.line_to(-80.0, 40.0);
            pb.close();
            pb.finish()
        }
        2 => Some(ellipse(-72.0, -72.0, 144.0, 144.0)),
        3 => Some(rect(-80.0, -30.0, 160.0, 60.0)),
        4 => Some(ellipse(-70.0, -70.0, 140.0, 140.0)),
        _ => stickers::shape(design),
    }
}

const FILMER_HEAD: &[(f32, f32)] = &[(-14.0, -56.0), (-2.0, -66.0), (12.0, -58.0), (12.0, -40.0), (0.0, -32.0), (-12.0, -38.0)];
const FILMER_HOODIE: &[(f32, f32)] = &[(-22.0, -30.0), (18.0, -32.0), (26.0, 10.0), (20.0, 16.0), (-22.0, 16.0), (-28.0, 6.0)];
const FILMER_PANTS: &[(f32, f32)] = &[(-22.0, 14.0), (20.0, 14.0), (24.0, 64.0), (4.0, 66.0), (0.0, 30.0), (-6.0, 66.0), (-26.0, 64.0)];
const FILMER_ARM: &[(f32, f32)] = &[(12.0, -26.0), (24.0, -40.0), (30.0, -56.0), (22.0, -60.0), (14.0, -46.0), (6.0, -30.0)];

/// `FilmerCut`: the filmer art's silhouette grown by a vinyl margin, holes filled, the outer edge traced.
fn filmer_cut() -> Vec<(f32, f32)> {
    let mut parts = Vec::new();
    for part in [FILMER_HEAD, FILMER_HOODIE, FILMER_PANTS, FILMER_ARM] {
        for ox in [0.0f32, 2.5] {
            let pts: Vec<(f32, f32)> = part.iter().map(|q| (q.0 + ox, q.1 + ox * 0.6)).collect();
            if let Some(p) = curve(&pts, 0.4, true) { parts.push(p); }
        }
    }
    parts.push(rect(14.0, -70.0, 30.0, 18.0));
    for (x, y, w, h) in [(38.0, -67.0, 10.0, 12.0), (-30.0, 62.0, 26.0, 12.0), (2.0, 62.0, 26.0, 12.0)] { parts.push(ellipse(x, y, w, h)); }
    die_cut(&parts)
}

/// A die-cut sticker edge round art made of several parts: their union grown by a vinyl margin, holes filled, the
/// outer edge traced (in the sticker's -120..120 space).
fn die_cut(parts: &[tiny_skia::Path]) -> Vec<(f32, f32)> {
    const K: u32 = 3;
    const N: u32 = 120 * 2 * K;
    const MARGIN: f32 = 6.0;
    let Some(mut bmp) = Pixmap::new(N, N) else { return Vec::new() };
    let ts = Transform::from_scale(K as f32, K as f32).pre_translate(120.0, 120.0);
    let black = { let mut p = paint(Color::BLACK); p.anti_alias = false; p };
    let pen = round_stroke(2.0 * MARGIN);
    let mut grow = |p: &tiny_skia::Path| { bmp.fill_path(p, &black, FillRule::Winding, ts, None); bmp.stroke_path(p, &black, &pen, ts, None); };
    for p in parts { grow(p); }
    let n = N as usize;
    let alpha: Vec<u8> = bmp.pixels().iter().map(|p| p.alpha()).collect();
    let mut outside = vec![false; n * n];
    let mut q = std::collections::VecDeque::new();
    for i in 0..n {
        for s in [i, (n - 1) * n + i, i * n, i * n + n - 1] {
            if !outside[s] && alpha[s] == 0 { outside[s] = true; q.push_back(s); }
        }
    }
    while let Some(c) = q.pop_front() {
        let (x, y) = (c % n, c / n);
        for nb in [if x > 0 { Some(c - 1) } else { None }, if x < n - 1 { Some(c + 1) } else { None }, if y > 0 { Some(c - n) } else { None }, if y < n - 1 { Some(c + n) } else { None }].into_iter().flatten() {
            if !outside[nb] && alpha[nb] == 0 { outside[nb] = true; q.push_back(nb); }
        }
    }
    let inside: Vec<bool> = outside.iter().map(|o| !o).collect();
    let Some(start) = inside.iter().position(|v| *v) else { return Vec::new() };
    let mx = [-1i64, -1, 0, 1, 1, 1, 0, -1];
    let my = [0i64, -1, -1, -1, 0, 1, 1, 1];
    let mut edge = Vec::new();
    let (mut cx, mut cy, mut dir) = ((start % n) as i64, (start / n) as i64, 3usize);
    loop {
        edge.push((cx, cy));
        let mut found = false;
        for k in 0..8 {
            let d = (dir + 5 + k) % 8;
            let (nx, ny) = (cx + mx[d], cy + my[d]);
            if nx >= 0 && ny >= 0 && nx < n as i64 && ny < n as i64 && inside[(ny as usize) * n + nx as usize] { cx = nx; cy = ny; dir = d; found = true; break; }
        }
        if !found || edge.len() > n * 8 { break; }
        if cx == (start % n) as i64 && cy == (start / n) as i64 { break; }
    }
    edge.iter().step_by(2 * K as usize).map(|(x, y)| (*x as f32 / K as f32 - 120.0, *y as f32 / K as f32 - 120.0)).collect()
}

/// `FitText`: the text as large as fits the rect, centred (GDI DrawString, no wrap).
#[allow(clippy::too_many_arguments)]
fn fit_text(px: &mut Pixmap, ts: Transform, clip: Option<&Mask>, s: &str, face: Option<&Face>, c: Color, r: (f32, f32, f32, f32)) {
    let Some(face) = face else { return };
    let (mw, mh) = face.measure(s, 100.0);
    let size = (r.3 * 100.0 / mh * 1.15).min(r.2 * 100.0 / mw).max(4.0);
    let (box_x, box_y, box_w, box_h) = (r.0 - 20.0, r.1 - size * 0.1, r.2 + 40.0, r.3 + size * 0.3);
    let (tw, th) = face.measure(s, size);
    let Some(path) = face.outline(s, size) else { return };
    let x = box_x + (box_w - tw) / 2.0 + size / 6.0;
    let y = box_y + (box_h - th) / 2.0;
    if let Some(p) = path.transform(Transform::from_translate(x, y)) {
        px.fill_path(&p, &paint(c), FillRule::Winding, ts, clip);
    }
}

/// `StickerArt`.
fn sticker_art(px: &mut Pixmap, ts: Transform, clip: Option<&Mask>, design: i32, variant: usize, rnd: &mut NetRandom) {
    let c = |r: u8, g: u8, b: u8| Color::from_rgba8(r, g, b, 255);
    let v = variant % VARIANTS;
    let fill = |px: &mut Pixmap, p: &tiny_skia::Path, c: Color| px.fill_path(p, &paint(c), FillRule::Winding, ts, clip);
    if design == 0 {
        fill(px, &rect(-90.0, -90.0, 180.0, 180.0), rgba(70, 255, 255, 255));
        let line = Color::from_rgba8(84, 58, 160, 255);
        let closed_fill = |px: &mut Pixmap, c: Color, pts: &[(f32, f32)]| {
            let off: Vec<(f32, f32)> = pts.iter().map(|p| (p.0 + 2.5, p.1 + 1.5)).collect();
            if let Some(p) = curve(&off, 0.4, true) { px.fill_path(&p, &paint(c), FillRule::EvenOdd, ts, clip); }
        };
        let (hoodie, pants, shoes) = [
            (c(232, 196, 64), c(150, 178, 214), c(150, 196, 80)),
            (c(214, 74, 60), c(70, 70, 82), c(240, 236, 226)),
            (c(118, 160, 92), c(196, 176, 130), c(226, 120, 60)),
            (c(232, 150, 190), c(122, 104, 176), c(250, 214, 80)),
        ][v];
        closed_fill(px, hoodie, FILMER_HOODIE);
        closed_fill(px, pants, FILMER_PANTS);
        closed_fill(px, Color::from_rgba8(250, 240, 222, 255), FILMER_HEAD);
        closed_fill(px, hoodie, FILMER_ARM);
        fill(px, &rect(-20.0, 6.0, 30.0, 12.0), Color::from_rgba8(170, 120, 170, 255));
        fill(px, &ellipse(-30.0, 62.0, 26.0, 12.0), shoes);
        fill(px, &ellipse(2.0, 62.0, 26.0, 12.0), shoes);
        fill(px, &rect(14.0, -70.0, 30.0, 18.0), Color::from_rgba8(58, 58, 66, 255));
        fill(px, &ellipse(38.0, -67.0, 10.0, 12.0), Color::from_rgba8(120, 150, 200, 255));
        let pen = Stroke { width: 2.4, line_join: LineJoin::Round, ..Default::default() };
        let (hd, ho, pa, ar) = (FILMER_HEAD, FILMER_HOODIE, FILMER_PANTS, FILMER_ARM);
        wobble(px, ts, clip, &pen, line, rnd, &[hd[0], hd[1], hd[2], hd[3], hd[4], hd[5], hd[0]]);
        wobble(px, ts, clip, &pen, line, rnd, &[(-14.0, -56.0), (-4.0, -68.0), (10.0, -62.0), (16.0, -58.0)]);
        wobble(px, ts, clip, &pen, line, rnd, &[ho[0], ho[5], ho[4], ho[3], ho[2], ho[1]]);
        wobble(px, ts, clip, &pen, line, rnd, &[pa[0], pa[6], pa[5], pa[4], pa[3], pa[2], pa[1]]);
        wobble(px, ts, clip, &pen, line, rnd, &[ar[0], ar[1], ar[2], ar[3], ar[4], ar[5]]);
        px.stroke_path(&rect(14.0, -70.0, 30.0, 18.0), &paint(line), &pen, ts, clip);
        wobble(px, ts, clip, &pen, line, rnd, &[(-2.0, -48.0), (2.0, -46.0), (4.0, -42.0)]);
        wobble(px, ts, clip, &pen, line, rnd, &[(-6.0, -30.0), (-4.0, 0.0), (-2.0, 12.0)]);
        px.stroke_path(&ellipse(-30.0, 62.0, 26.0, 12.0), &paint(line), &pen, ts, clip);
        px.stroke_path(&ellipse(2.0, 62.0, 26.0, 12.0), &paint(line), &pen, ts, clip);
        if let Some(sh) = sticker_shape(0) { px.stroke_path(&sh, &paint(rgba(130, 255, 255, 255)), &stroke(1.5), ts, clip); }
        return;
    }
    let cream = Color::from_rgba8(240, 234, 218, 255);
    fill(px, &rect(-90.0, -90.0, 180.0, 180.0), cream);
    let arial_black = Face::load(&["ariblk.ttf", "DejaVuSans-Bold.ttf", "Arial Black.ttf"]);
    let tahoma_bold = Face::load(&["tahomabd.ttf", "DejaVuSans-Bold.ttf", "Tahoma Bold.ttf"]);
    match design {
        1 => {
            let mut pb = PathBuilder::new();
            if let Some(a) = arc(-72.0, -50.0, 144.0, 60.0, 180.0, 180.0) {
                let pts = points(&a);
                pb.move_to(pts[0].0, pts[0].1);
                for p in &pts[1..] { pb.line_to(p.0, p.1); }
                pb.line_to(72.0, -20.0);
                pb.line_to(72.0, 33.0);
                pb.line_to(-72.0, 33.0);
                pb.close();
                if let Some(inner) = pb.finish() { fill(px, &inner, [c(150, 132, 88), c(40, 52, 84), c(30, 30, 32), c(92, 100, 62)][v]); }
            }
            fit_text(px, ts, clip, "BLICKEY", arial_black.as_ref(), cream, (-64.0, -20.0, 128.0, 30.0));
            let line = ["SINCE 1922 (PROBABLY)", "WORKWEAR-ISH", "EST. WHENEVER", "HEAVY DUTY-ISH"][v];
            fit_text(px, ts, clip, line, tahoma_bold.as_ref(), cream, (-56.0, 14.0, 112.0, 12.0));
        }
        2 => {
            let (ring, rays) = [(c(214, 52, 38), c(252, 196, 60)), (c(36, 96, 196), c(240, 236, 226)), (c(52, 150, 72), c(250, 226, 70)), (c(120, 60, 160), c(250, 150, 50))][v];
            fill(px, &ellipse(-64.0, -64.0, 128.0, 128.0), ring);
            for i in 0..9 {
                let a = -std::f64::consts::FRAC_PI_2 + (i as f64 - 4.0) * 0.28;
                let p = |ang: f64, r: f64| ((ang.cos() * r) as f32, (ang.sin() * r) as f32 - 6.0);
                if let Some(t) = poly(&[p(a - 0.12, 26.0), p(a, 54.0), p(a + 0.12, 26.0)], true) { fill(px, &t, rays); }
            }
            fill(px, &ellipse(-24.0, -28.0, 48.0, 48.0), cream);
            let dark = Color::from_rgba8(40, 30, 30, 255);
            px.stroke_path(&ellipse(-24.0, -28.0, 48.0, 48.0), &paint(dark), &stroke(3.0), ts, clip);
            if let Some(a) = arc(-10.0, -6.0, 20.0, 14.0, 10.0, 160.0) { px.stroke_path(&a, &paint(dark), &stroke(3.0), ts, clip); }
            fill(px, &ellipse(-11.0, -14.0, 6.0, 8.0), dark);
            fill(px, &ellipse(5.0, -14.0, 6.0, 8.0), dark);
            let line = ["SPITBALL WHEELS", "SPITBALL 99A", "SPITBALL CONICAL", "SPITBALL FORMULA"][v];
            fit_text(px, ts, clip, line, arial_black.as_ref(), cream, (-38.0, 26.0, 76.0, 15.0));
        }
        3 => {
            let (bg, ink, word) = [(c(222, 30, 36), Color::WHITE, "Supremely Mid"), (c(24, 24, 26), Color::WHITE, "Supremely Mid"), (Color::WHITE, c(222, 30, 36), "Supremely Mid"), (c(222, 30, 36), Color::WHITE, "Mostly Mid")][v];
            fill(px, &rect(-74.0, -24.0, 148.0, 48.0), bg);
            let arial_bi = Face::load(&["arialbi.ttf", "DejaVuSans-BoldOblique.ttf", "Arial Bold Italic.ttf"]);
            fit_text(px, ts, clip, word, arial_bi.as_ref(), ink, (-68.0, -18.0, 136.0, 36.0));
        }
        4 => {
            let (base, flame, title) = [(c(20, 20, 22), c(240, 110, 30), c(255, 226, 120)), (c(20, 20, 22), c(236, 70, 150), c(250, 250, 250)), (c(236, 232, 222), c(36, 110, 220), c(24, 24, 26)), (c(20, 20, 22), c(110, 200, 70), c(255, 226, 120))][v];
            fill(px, &ellipse(-62.0, -62.0, 124.0, 124.0), base);
            for i in 0..7 {
                let x = -48.0 + i as f32 * 16.0;
                if let Some(t) = poly(&[(x - 7.0, 10.0), (x + 2.0, -30.0 - (i % 3) as f32 * 8.0), (x + 9.0, 10.0)], true) { fill(px, &t, flame); }
            }
            let impact = Face::load(&["impact.ttf", "DejaVuSans-Bold.ttf", "Impact.ttf"]);
            fit_text(px, ts, clip, "THRASHED", impact.as_ref(), title, (-54.0, -12.0, 108.0, 30.0));
            let line = ["SKATE AND REST", "SKATE AND NAP", "SKATE AND SNACK", "MAG SINCE '81"][v];
            let under = if v == 2 { c(24, 24, 26) } else { cream };
            fit_text(px, ts, clip, line, tahoma_bold.as_ref(), under, (-40.0, 22.0, 80.0, 12.0));
        }
        _ => stickers::art(px, ts, clip, design, v, rnd),
    }
}

/// `DrawSticker`: the sticker aged (faded, scuffed, chipped) with one edge peeled back over a fold line.
/// `preview`: the design flat and clean (sticker sheet, the Tools picker): no peel, scuffs or weathering.
#[allow(clippy::too_many_arguments)]
fn draw_sticker(px: &mut Pixmap, design: i32, variant: usize, pattern: i32, ridges: &[[(f32, f32); 2]], ts: Transform, preview: bool) {
    let seed = pattern.rem_euclid(WEAR_PATTERNS) * 7919 + 101;
    let mut rnd = NetRandom::new(seed);
    let Some(shape) = sticker_shape(design) else { return };
    let bounds = shape.bounds();
    let pa = rnd.next_double() * std::f64::consts::PI * 2.0;
    let (nx, ny) = (pa.cos() as f32, pa.sin() as f32);
    let reach = nx.abs() * bounds.width() / 2.0 + ny.abs() * bounds.height() / 2.0;
    let dist = reach * if preview { 10.0 } else { 0.74 + 0.14 * rnd.next_double() as f32 };
    let p = (nx * dist, ny * dist);
    let (dx, dy) = (-ny, nx);
    let Some(half) = poly(&[(p.0 + dx * 400.0, p.1 + dy * 400.0), (p.0 - dx * 400.0, p.1 - dy * 400.0), (p.0 - dx * 400.0 + nx * 400.0, p.1 - dy * 400.0 + ny * 400.0), (p.0 + dx * 400.0 + nx * 400.0, p.1 + dy * 400.0 + ny * 400.0)], true) else { return };
    let shape_m = mask_of(&shape, ts);
    let half_m = mask_of(&half, ts);
    let kept = and_not(&shape_m, &half_m);
    let lifted = and(&shape_m, &half_m);
    // Shadow of the kept part, offset (2, 3) in the sticker's space.
    let shadow_ts = ts.pre_translate(2.0, 3.0);
    let kept_shadow = and_not(&mask_of(&shape, shadow_ts), &mask_of(&half, shadow_ts));
    px.fill_rect(tiny_skia::Rect::from_xywh(0.0, 0.0, W as f32, H as f32).expect("rect"), &paint(rgba(70, 0, 0, 0)), Transform::identity(), Some(&kept_shadow));
    sticker_art(px, ts, Some(&kept), design, variant, &mut rnd);
    if preview { return; }
    for _ in 0..16 {
        let (x0, y0) = ((rnd.next_double() * 160.0 - 80.0) as f32, (rnd.next_double() * 160.0 - 80.0) as f32);
        let (x1, y1) = (x0 + (rnd.next_double() * 40.0 - 20.0) as f32, y0 + (rnd.next_double() * 12.0 - 6.0) as f32);
        if let Some(l) = poly(&[(x0, y0), (x1, y1)], false) { px.stroke_path(&l, &paint(rgba(110, 255, 255, 250)), &stroke(1.2), ts, Some(&kept)); }
    }
    for _ in 0..40 {
        let (x, y) = ((rnd.next_double() * 160.0 - 80.0) as f32, (rnd.next_double() * 160.0 - 80.0) as f32);
        px.fill_path(&ellipse(x, y, 2.5, 2.5), &paint(rgba(60, 60, 50, 30)), FillRule::Winding, ts, Some(&kept));
    }
    px.fill_path(&rect(-90.0, -90.0, 180.0, 180.0), &paint(rgba(30, 120, 110, 80)), FillRule::Winding, ts, Some(&kept));
    creases(px, ts, Some(&kept), ridges);
    for _ in 0..4 {
        let ca = rnd.next_double() * std::f64::consts::PI * 2.0;
        let (ex, ey) = ((ca.cos() as f32) * bounds.width() * 0.47, (ca.sin() as f32) * bounds.height() * 0.47);
        if let Some(c) = poly(&[(ex - 6.0, ey - 3.0), (ex + 5.0, ey - 5.0), (ex + 7.0, ey + 4.0), (ex - 3.0, ey + 7.0)], true) {
            px.fill_path(&c, &paint(rgba(120, 46, 47, 52)), FillRule::Winding, ts, Some(&kept));
        }
    }
    sticker_weathering(px, &kept, seed);
    px.fill_rect(tiny_skia::Rect::from_xywh(0.0, 0.0, W as f32, H as f32).expect("rect"), &paint(rgba(60, 210, 196, 150)), Transform::identity(), Some(&lifted));
    // The flap folded back over the line: the lifted part mirrored about it.
    let (c2, s2) = (dx * dx - dy * dy, 2.0 * dx * dy);
    let mirror = Transform::from_row(c2, s2, s2, -c2, p.0 - (c2 * p.0 + s2 * p.1), p.1 - (s2 * p.0 - c2 * p.1));
    let flap_ts = ts.pre_concat(mirror);
    let flap = |t: Transform| and(&mask_of(&shape, t), &mask_of(&half, t));
    let shadow = flap(flap_ts.pre_translate(-nx * 3.0, -ny * 3.0));
    px.fill_rect(tiny_skia::Rect::from_xywh(0.0, 0.0, W as f32, H as f32).expect("rect"), &paint(rgba(80, 0, 0, 0)), Transform::identity(), Some(&shadow));
    let face = flap(flap_ts);
    let ang = (pa * 180.0 / std::f64::consts::PI) as f32;
    let (gx, gy) = (ang.to_radians().cos() * 90.0, ang.to_radians().sin() * 90.0);
    if let Some(sh) = tiny_skia::LinearGradient::new(Point::from_xy(-gx, -gy), Point::from_xy(gx, gy),
        vec![tiny_skia::GradientStop::new(0.0, Color::from_rgba8(238, 234, 222, 255)), tiny_skia::GradientStop::new(1.0, Color::from_rgba8(196, 190, 176, 255))], tiny_skia::SpreadMode::Pad, flap_ts) {
        let mut pt = Paint::default();
        pt.shader = sh;
        pt.anti_alias = true;
        px.fill_rect(tiny_skia::Rect::from_xywh(0.0, 0.0, W as f32, H as f32).expect("rect"), &pt, Transform::identity(), Some(&face));
    }
    let shape_clip = mask_of(&shape, ts);
    if let Some(l) = poly(&[(p.0 + dx * 120.0, p.1 + dy * 120.0), (p.0 - dx * 120.0, p.1 - dy * 120.0)], false) {
        px.stroke_path(&l, &paint(rgba(90, 0, 0, 0)), &stroke(1.2), ts, Some(&shape_clip));
    }
}
