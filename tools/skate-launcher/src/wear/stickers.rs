//! The skate-culture parody stickers past the first five (doc 44a 6): die-cut shapes and art, four variations each.
use tiny_skia::{Color, FillRule, Mask, Path, PathBuilder, Pixmap, Transform};

use super::{ellipse, fit_text, paint, poly, rect, round_rect, round_stroke, Face};

/// Designs this file draws: `FIRST..FIRST + COUNT`.
pub const FIRST: i32 = 5;
pub const COUNT: i32 = 25;

pub fn shape(design: i32) -> Option<Path> {
    Some(match design {
        5 => {
            let mut pb = PathBuilder::new();
            pb.move_to(-62.0, -70.0);
            pb.line_to(62.0, -70.0);
            pb.line_to(62.0, -4.0);
            pb.quad_to(56.0, 46.0, 0.0, 74.0);
            pb.quad_to(-56.0, 46.0, -62.0, -4.0);
            pb.close();
            pb.finish()?
        }
        6 => poly(&[(-72.0, -58.0), (72.0, -58.0), (0.0, 76.0)], true)?,
        7 => ellipse(-80.0, -46.0, 160.0, 92.0),
        8 => rect(-80.0, -34.0, 160.0, 68.0),
        9 | 14 => ellipse(-66.0, -66.0, 132.0, 132.0),
        10 => ellipse(-68.0, -68.0, 136.0, 136.0),
        11 => rect(-86.0, -28.0, 172.0, 56.0),
        12 => round_rect(-78.0, -40.0, 156.0, 80.0, 8.0),
        15 => round_rect(-80.0, -46.0, 160.0, 92.0, 16.0),
        16 => rect(-80.0, -30.0, 160.0, 60.0),
        _ => return shape2(design),
    })
}

fn inner(design: i32, k: f32) -> Option<Path> {
    shape(design)?.transform(Transform::from_scale(k, k))
}

fn c(r: u8, g: u8, b: u8) -> Color {
    Color::from_rgba8(r, g, b, 255)
}

pub fn art(px: &mut Pixmap, ts: Transform, clip: Option<&Mask>, design: i32, v: usize, rnd: &mut crate::camcorder::NetRandom) {
    let fill = |px: &mut Pixmap, p: &Path, col: Color| px.fill_path(p, &paint(col), FillRule::Winding, ts, clip);
    let line = |px: &mut Pixmap, pts: &[(f32, f32)], w: f32, col: Color| {
        if let Some(p) = poly(pts, false) { px.stroke_path(&p, &paint(col), &round_stroke(w), ts, clip); }
    };
    let text = |px: &mut Pixmap, s: &str, f: &Option<Face>, col: Color, r: (f32, f32, f32, f32)| fit_text(px, ts, clip, s, f.as_ref(), col, r);
    let black = Face::load(&["ariblk.ttf", "DejaVuSans-Bold.ttf", "Arial Black.ttf"]);
    let bold = Face::load(&["tahomabd.ttf", "DejaVuSans-Bold.ttf", "Tahoma Bold.ttf"]);
    let impact = Face::load(&["impact.ttf", "DejaVuSans-Bold.ttf", "Impact.ttf"]);
    let hand = Face::load(&["Inkfree.ttf", "segoepr.ttf", "comic.ttf", "DejaVuSans.ttf"]);
    let cream = c(240, 234, 218);
    let ink = c(26, 26, 28);
    match design {
        // Codependent Truck Co.: a shield.
        5 => {
            let (bg, fg) = [(c(200, 36, 36), cream), (ink, cream), (c(32, 48, 92), cream), (c(232, 186, 60), ink)][v];
            if let Some(p) = inner(5, 0.86) { fill(px, &p, bg); }
            fill(px, &ellipse(-44.0, -54.0, 8.0, 8.0), fg);
            fill(px, &ellipse(36.0, -54.0, 8.0, 8.0), fg);
            text(px, "CODEPENDENT", &impact, fg, (-50.0, -32.0, 100.0, 24.0));
            text(px, "TRUCK CO.", &bold, fg, (-34.0, 2.0, 68.0, 14.0));
        }
        // Santa Crust: a pizza slice.
        6 => {
            let (cheese, topping) = [(c(250, 204, 80), c(196, 46, 36)), (c(250, 214, 110), c(130, 120, 110)), (c(244, 196, 70), c(30, 30, 30)), (c(250, 210, 96), c(90, 160, 60))][v];
            if let Some(p) = poly(&[(-60.0, -48.0), (60.0, -48.0), (0.0, 64.0)], true) { fill(px, &p, cheese); }
            if let Some(p) = poly(&[(-60.0, -48.0), (60.0, -48.0), (53.0, -35.0), (-53.0, -35.0)], true) { fill(px, &p, c(176, 112, 52)); }
            for (x, y, r) in [(-16.0, -2.0, 7.0), (10.0, 10.0, 7.0), (-2.0, 30.0, 6.0), (22.0, -10.0, 5.0)] { fill(px, &ellipse(x - r, y - r, 2.0 * r, 2.0 * r), topping); }
            text(px, "SANTA CRUST", &impact, ink, (-38.0, -31.0, 76.0, 16.0));
        }
        // Faker: a signature on an oval.
        7 => {
            let (bg, fg) = [(cream, ink), (ink, cream), (c(200, 36, 36), cream), (c(250, 214, 60), ink)][v];
            if let Some(p) = inner(7, 0.9) { fill(px, &p, bg); }
            text(px, "Faker", &hand, fg, (-56.0, -28.0, 112.0, 40.0));
            line(px, &[(-46.0, 20.0), (-10.0, 24.0), (30.0, 18.0), (50.0, 12.0)], 3.0, fg);
        }
        // Vents: Off The Mall, a checkerboard tab.
        8 => {
            let bg = [c(196, 36, 36), c(32, 48, 92), ink, c(40, 140, 140)][v];
            fill(px, &rect(-74.0, -28.0, 148.0, 56.0), bg);
            for row in 0..6 {
                for col in 0..6 {
                    let sq = if (row + col) % 2 == 0 { ink } else { cream };
                    fill(px, &rect(-70.0 + col as f32 * 9.0, -27.0 + row as f32 * 9.0, 9.0, 9.0), sq);
                }
            }
            text(px, "VENTS", &black, cream, (-8.0, -24.0, 76.0, 24.0));
            text(px, "OFF THE MALL", &bold, cream, (-8.0, 6.0, 76.0, 12.0));
        }
        // Grizzled Grip: a paw.
        9 => {
            let (bg, paw) = [(ink, cream), (c(96, 64, 40), cream), (c(60, 100, 60), ink), (cream, ink)][v];
            if let Some(p) = inner(9, 0.9) { fill(px, &p, bg); }
            fill(px, &ellipse(-24.0, -10.0, 48.0, 38.0), paw);
            for (x, y) in [(-34.0, -30.0), (-14.0, -44.0), (8.0, -44.0), (26.0, -30.0)] { fill(px, &ellipse(x - 1.0, y, 14.0, 16.0), paw); }
            text(px, "GRIZZLED GRIP", &bold, paw, (-34.0, 34.0, 68.0, 11.0));
        }
        // No Skating?: the sign, slashed.
        10 => {
            let (bg, ring) = [(cream, c(210, 36, 36)), (cream, c(236, 120, 30)), (c(250, 214, 60), ink), (cream, c(36, 96, 196))][v];
            if let Some(p) = inner(10, 0.92) { fill(px, &p, bg); }
            px.stroke_path(&ellipse(-54.0, -54.0, 108.0, 108.0), &paint(ring), &round_stroke(12.0), ts, clip);
            fill(px, &round_rect(-38.0, -10.0, 76.0, 14.0, 7.0), ink);
            for x in [-30.0, 20.0] { fill(px, &ellipse(x, 6.0, 10.0, 10.0), ink); }
            line(px, &[(-38.0, -38.0), (38.0, 38.0)], 11.0, ring);
            text(px, "NO SKATING?", &bold, ink, (-24.0, 24.0, 48.0, 9.0));
        }
        // A bumper sticker.
        11 => {
            let (bg, fg) = [(c(250, 214, 60), ink), (ink, cream), (cream, c(200, 36, 36)), (c(40, 120, 64), cream)][v];
            fill(px, &rect(-82.0, -24.0, 164.0, 48.0), bg);
            text(px, "SKATEBOARDING IS NOT A CRIME", &black, fg, (-78.0, -20.0, 156.0, 20.0));
            text(px, "(MOSTLY)", &bold, fg, (-30.0, 4.0, 60.0, 12.0));
        }
        // Auntie Hero: a bird and a name.
        12 => {
            let (bg, fg) = [(ink, cream), (cream, ink), (c(200, 36, 36), ink), (c(32, 48, 92), cream)][v];
            fill(px, &rect(-74.0, -36.0, 148.0, 72.0), bg);
            if let Some(p) = poly(&[(-40.0, -26.0), (0.0, -6.0), (40.0, -26.0), (30.0, -8.0), (0.0, 6.0), (-30.0, -8.0)], true) { fill(px, &p, fg); }
            fill(px, &ellipse(-5.0, -14.0, 10.0, 10.0), fg);
            text(px, "AUNTIE HERO", &impact, fg, (-54.0, 6.0, 108.0, 18.0));
        }
        // Shake Junk: two words and stars.
        14 => {
            let (bg, fg) = [(c(40, 150, 64), c(250, 226, 70)), (ink, c(110, 200, 70)), (c(250, 214, 60), ink), (c(110, 50, 150), c(140, 230, 90))][v];
            if let Some(p) = inner(14, 0.9) { fill(px, &p, bg); }
            text(px, "SHAKE", &impact, fg, (-42.0, -30.0, 84.0, 24.0));
            text(px, "JUNK", &impact, fg, (-32.0, 0.0, 64.0, 24.0));
            for (x, y) in [(-40.0, 30.0), (32.0, 30.0), (0.0, -46.0)] {
                if let Some(p) = poly(&[(x, y - 6.0), (x + 2.0, y - 1.0), (x + 7.0, y), (x + 2.0, y + 2.0), (x, y + 7.0), (x - 2.0, y + 2.0), (x - 7.0, y), (x - 2.0, y - 1.0)], true) { fill(px, &p, fg); }
            }
        }
        // Support your local skate shop.
        15 => {
            let (bg, fg) = [(c(32, 48, 92), cream), (cream, ink), (c(200, 36, 36), cream), (c(40, 120, 64), cream)][v];
            if let Some(p) = inner(15, 0.9) { fill(px, &p, bg); }
            fill(px, &round_rect(-38.0, -36.0, 76.0, 12.0, 6.0), fg);
            for x in [-30.0, -20.0, 12.0, 22.0] { fill(px, &ellipse(x, -24.0, 8.0, 8.0), fg); }
            text(px, "SUPPORT YOUR", &bold, fg, (-46.0, -12.0, 92.0, 12.0));
            text(px, "LOCAL SKATE SHOP", &black, fg, (-62.0, 6.0, 124.0, 18.0));
        }
        // Surreal: a boxed word.
        16 => {
            let (bg, fg) = [(cream, c(200, 36, 36)), (ink, cream), (c(200, 36, 36), cream), (c(36, 96, 196), cream)][v];
            fill(px, &rect(-76.0, -26.0, 152.0, 52.0), bg);
            px.stroke_path(&rect(-70.0, -22.0, 140.0, 44.0), &paint(fg), &round_stroke(3.0), ts, clip);
            text(px, "SURREAL", &black, fg, (-56.0, -19.0, 112.0, 20.0));
            text(px, "SKATEBOARDS", &bold, fg, (-26.0, 6.0, 52.0, 8.0));
        }
        _ => art2(px, ts, clip, design, v, rnd),
    }
}

/// Designs past Surreal, and the evil eye in slot 13: the set that leans on the looks and jokes many women skaters pick
/// (cute with an edge, Y2K, illustrated, brand parodies), rolled about as often as the rest.
pub const SOFT: [i32; 14] = [13, 17, 18, 19, 20, 21, 22, 23, 24, 25, 26, 27, 28, 29];

fn rot(p: Path, deg: f32, cx: f32, cy: f32) -> Option<Path> {
    p.transform(Transform::from_rotate_at(deg, cx, cy))
}

fn heart(k: f32) -> Option<Path> {
    let mut pb = PathBuilder::new();
    pb.move_to(0.0, 62.0 * k);
    pb.cubic_to(-96.0 * k, 4.0 * k, -64.0 * k, -74.0 * k, 0.0, -34.0 * k);
    pb.cubic_to(64.0 * k, -74.0 * k, 96.0 * k, 4.0 * k, 0.0, 62.0 * k);
    pb.close();
    pb.finish()
}

fn star(cx: f32, cy: f32, r: f32, inner: f32, points: usize) -> Option<Path> {
    let pts: Vec<(f32, f32)> = (0..points * 2).map(|i| {
        let a = -std::f32::consts::FRAC_PI_2 + i as f32 * std::f32::consts::PI / points as f32;
        let rr = if i % 2 == 0 { r } else { r * inner };
        (cx + a.cos() * rr, cy + a.sin() * rr)
    }).collect();
    poly(&pts, true)
}

fn ring_pts(cx: f32, cy: f32, rx: f32, ry: f32, from: f32, to: f32, n: usize) -> Vec<(f32, f32)> {
    (0..=n).map(|i| { let a = (from + (to - from) * i as f32 / n as f32).to_radians(); (cx + a.cos() * rx, cy + a.sin() * ry) }).collect()
}

/// Upper half annulus (a rainbow band).
fn band(r0: f32, r1: f32) -> Option<Path> {
    let mut pts = ring_pts(0.0, 30.0, r1, r1, 180.0, 360.0, 40);
    pts.extend(ring_pts(0.0, 30.0, r0, r0, 360.0, 180.0, 40));
    poly(&pts, true)
}

const SK_HAIR: &[(f32, f32)] = &[(-14.0, -62.0), (14.0, -62.0), (20.0, -46.0), (22.0, -20.0), (25.0, -4.0), (12.0, -8.0), (9.0, -29.0), (-9.0, -29.0), (-12.0, -8.0), (-25.0, -4.0), (-22.0, -20.0), (-20.0, -46.0)];
const SK_HEAD: &[(f32, f32)] = &[(-12.0, -56.0), (0.0, -63.0), (12.0, -56.0), (11.0, -41.0), (0.0, -33.0), (-11.0, -41.0)];
const SK_BEANIE: &[(f32, f32)] = &[(-15.0, -55.0), (-12.0, -68.0), (0.0, -75.0), (12.0, -68.0), (15.0, -55.0)];
const SK_TOP: &[(f32, f32)] = &[(-20.0, -30.0), (18.0, -30.0), (24.0, 8.0), (-24.0, 8.0)];
const SK_PANTS: &[(f32, f32)] = &[(-24.0, 6.0), (22.0, 6.0), (28.0, 64.0), (6.0, 66.0), (2.0, 28.0), (-4.0, 66.0), (-28.0, 64.0)];
const SK_ARM: &[(f32, f32)] = &[(14.0, -28.0), (22.0, -24.0), (36.0, 2.0), (30.0, 8.0), (12.0, -10.0)];

/// The parts each die-cut design's edge is traced round (drawn by `art2` in the same places).
fn parts(design: i32) -> Vec<Path> {
    let mut v: Vec<Option<Path>> = Vec::new();
    match design {
        18 => {
            v.push(Some(ellipse(-52.0, 0.0, 48.0, 48.0)));
            v.push(Some(ellipse(2.0, 8.0, 48.0, 48.0)));
            v.push(poly(&[(-30.0, 2.0), (-25.0, 2.0), (8.0, -50.0), (3.0, -50.0)], true));
            v.push(poly(&[(24.0, 10.0), (29.0, 10.0), (8.0, -50.0), (3.0, -50.0)], true));
            v.push(rot(ellipse(4.0, -66.0, 42.0, 18.0), -20.0, 25.0, -57.0));
            v.push(Some(rect(-56.0, 54.0, 112.0, 18.0)));
        }
        20 => {
            v.push(rot(ellipse(-76.0, -52.0, 72.0, 56.0), -20.0, -40.0, -24.0));
            v.push(rot(ellipse(4.0, -52.0, 72.0, 56.0), 20.0, 40.0, -24.0));
            v.push(rot(ellipse(-60.0, -2.0, 54.0, 46.0), 20.0, -33.0, 21.0));
            v.push(rot(ellipse(6.0, -2.0, 54.0, 46.0), -20.0, 33.0, 21.0));
            v.push(Some(round_rect(-5.0, -42.0, 10.0, 80.0, 5.0)));
            v.push(Some(rect(-52.0, 48.0, 104.0, 16.0)));
        }
        23 => {
            v.push(Some(ellipse(-62.0, -40.0, 124.0, 100.0)));
            v.push(poly(&[(-58.0, -14.0), (-48.0, -72.0), (-12.0, -40.0)], true));
            v.push(poly(&[(58.0, -14.0), (48.0, -72.0), (12.0, -40.0)], true));
        }
        24 => {
            for i in 0..10 { v.push(rot(ellipse(-12.0, -76.0, 24.0, 56.0), i as f32 * 36.0, 0.0, 0.0)); }
            v.push(Some(ellipse(-30.0, -30.0, 60.0, 60.0)));
        }
        25 => {
            v.push(super::curve(&[(-56.0, -30.0), (56.0, -30.0), (46.0, 20.0), (0.0, 70.0), (-46.0, 20.0)], 0.5, true));
            for i in 0..5 { v.push(rot(ellipse(-7.0, -60.0, 14.0, 30.0), -60.0 + i as f32 * 30.0, 0.0, -34.0)); }
        }
        27 => {
            for part in [SK_HAIR, SK_HEAD, SK_BEANIE, SK_TOP, SK_PANTS, SK_ARM] {
                for ox in [0.0f32, 2.5] {
                    let pts: Vec<(f32, f32)> = part.iter().map(|q| (q.0 + ox, q.1 + ox * 0.6)).collect();
                    v.push(super::curve(&pts, 0.4, true));
                }
            }
            v.push(Some(round_rect(30.0, -44.0, 16.0, 104.0, 8.0)));
            for (x, y, w, h) in [(-32.0, 62.0, 26.0, 12.0), (4.0, 62.0, 26.0, 12.0)] { v.push(Some(ellipse(x, y, w, h))); }
        }
        29 => {
            v.push(poly(&[(-34.0, 8.0), (34.0, 8.0), (0.0, 80.0)], true));
            v.push(Some(ellipse(-48.0, -54.0, 96.0, 72.0)));
            for (x, h) in [(-40.0, 22.0), (-10.0, 28.0), (20.0, 18.0)] { v.push(Some(round_rect(x, 0.0, 14.0, h, 7.0))); }
            v.push(Some(ellipse(-8.0, -70.0, 16.0, 16.0)));
        }
        _ => {}
    }
    v.into_iter().flatten().collect()
}

/// Die-cut edges are traced once per design.
fn cut(design: i32) -> Option<Path> {
    static CUTS: std::sync::Mutex<Vec<(i32, Vec<(f32, f32)>)>> = std::sync::Mutex::new(Vec::new());
    let mut cache = CUTS.lock().ok()?;
    if !cache.iter().any(|(d, _)| *d == design) {
        let pts = super::die_cut(&parts(design));
        cache.push((design, pts));
    }
    let pts = &cache.iter().find(|(d, _)| *d == design)?.1;
    super::curve(pts, 0.5, true)
}

fn shape2(design: i32) -> Option<Path> {
    match design {
        13 | 21 => Some(ellipse(-80.0, -46.0, 160.0, 92.0)),
        17 => heart(1.0),
        19 => star(0.0, 0.0, 84.0, 0.32, 4).and_then(|p| p.transform(Transform::identity())),
        22 => {
            let mut pts = ring_pts(0.0, 30.0, 80.0, 84.0, 180.0, 360.0, 48);
            pts.push((80.0, 48.0));
            pts.push((-80.0, 48.0));
            poly(&pts, true)
        }
        26 | 28 => Some(ellipse(-66.0, -66.0, 132.0, 132.0)),
        _ => cut(design),
    }
}

#[allow(clippy::too_many_lines)]
fn art2(px: &mut Pixmap, ts: Transform, clip: Option<&Mask>, design: i32, v: usize, rnd: &mut crate::camcorder::NetRandom) {
    let fill = |px: &mut Pixmap, p: &Path, col: Color| px.fill_path(p, &paint(col), FillRule::Winding, ts, clip);
    let fill_o = |px: &mut Pixmap, p: Option<Path>, col: Color| if let Some(p) = p { px.fill_path(&p, &paint(col), FillRule::Winding, ts, clip); };
    let line = |px: &mut Pixmap, pts: &[(f32, f32)], w: f32, col: Color| {
        if let Some(p) = poly(pts, false) { px.stroke_path(&p, &paint(col), &round_stroke(w), ts, clip); }
    };
    let text = |px: &mut Pixmap, s: &str, f: &Option<Face>, col: Color, r: (f32, f32, f32, f32)| fit_text(px, ts, clip, s, f.as_ref(), col, r);
    let black = Face::load(&["ariblk.ttf", "DejaVuSans-Bold.ttf", "Arial Black.ttf"]);
    let bold = Face::load(&["tahomabd.ttf", "DejaVuSans-Bold.ttf", "Tahoma Bold.ttf"]);
    let hand = Face::load(&["Inkfree.ttf", "segoepr.ttf", "comic.ttf", "DejaVuSans.ttf"]);
    let cream = c(240, 234, 218);
    let ink = c(26, 26, 28);
    let pk = parts(design);
    match design {
        // Evil eye: no bad vibes.
        13 => {
            let (bg, outer, iris) = [(c(30, 60, 140), cream, c(110, 180, 230)), (c(232, 150, 190), cream, c(70, 120, 200)), (ink, c(250, 214, 60), c(110, 180, 230)), (c(130, 100, 190), cream, c(120, 220, 200))][v];
            fill_o(px, inner(13, 0.9), bg);
            fill(px, &ellipse(-28.0, -38.0, 56.0, 56.0), outer);
            fill(px, &ellipse(-20.0, -30.0, 40.0, 40.0), iris);
            fill(px, &ellipse(-10.0, -20.0, 20.0, 20.0), ink);
            fill(px, &ellipse(-1.0, -17.0, 6.0, 6.0), cream);
            text(px, "NO BAD VIBES", &bold, outer, (-30.0, 23.0, 60.0, 8.0));
        }
        // A heart with a band-aid.
        17 => {
            let (bg, aid) = [(c(232, 90, 140), c(232, 200, 160)), (c(200, 36, 36), c(232, 200, 160)), (c(170, 140, 220), c(250, 236, 200)), (ink, c(250, 170, 200))][v];
            fill_o(px, heart(0.86), bg);
            let tape = |p: Path| rot(p, -28.0, 0.0, -12.0);
            fill_o(px, tape(round_rect(-46.0, -24.0, 92.0, 24.0, 12.0)), aid);
            fill_o(px, tape(rect(-12.0, -22.0, 24.0, 20.0)), c(250, 240, 230));
            for (x, y) in [(-34.0, -15.0), (-28.0, -9.0), (26.0, -15.0), (32.0, -9.0)] { fill_o(px, tape(ellipse(x, y, 3.0, 3.0)), c(190, 150, 110)); }
            text(px, "SLAM HAPPENS", &black, cream, (-30.0, 14.0, 60.0, 10.0));
        }
        // Cherry bomb.
        18 => {
            let (fruit, ribbon, word) = [(c(200, 30, 46), ink, cream), (c(236, 80, 150), c(30, 30, 30), c(250, 200, 220)), (c(90, 20, 60), c(232, 150, 190), ink), (c(240, 96, 80), c(30, 60, 140), cream)][v];
            let leaf = c(80, 150, 70);
            fill(px, &pk[2], c(70, 110, 50));
            fill(px, &pk[3], c(70, 110, 50));
            fill(px, &pk[4], leaf);
            fill(px, &pk[0], fruit);
            fill(px, &pk[1], fruit);
            fill(px, &ellipse(-40.0, 8.0, 12.0, 8.0), c(255, 220, 220));
            fill(px, &ellipse(14.0, 16.0, 12.0, 8.0), c(255, 220, 220));
            fill(px, &pk[5], ribbon);
            text(px, "CHERRY BOMB", &black, word, (-50.0, 56.0, 100.0, 13.0));
        }
        // Y2K chrome sparkle.
        19 => {
            let stops = [
                [c(150, 156, 170), c(250, 252, 255), c(110, 116, 130), c(220, 226, 240)],
                [c(240, 150, 210), c(250, 250, 255), c(140, 180, 250), c(200, 160, 250)],
                [c(190, 140, 50), c(255, 240, 180), c(150, 100, 30), c(240, 200, 110)],
                [c(30, 30, 36), c(150, 150, 170), c(20, 20, 24), c(90, 90, 110)],
            ][v];
            if let (Some(p), Some(sh)) = (star(0.0, 0.0, 72.0, 0.32, 4), tiny_skia::LinearGradient::new(
                tiny_skia::Point::from_xy(-60.0, -70.0), tiny_skia::Point::from_xy(60.0, 70.0),
                stops.iter().enumerate().map(|(i, s)| tiny_skia::GradientStop::new(i as f32 / 3.0, *s)).collect(),
                tiny_skia::SpreadMode::Pad, Transform::identity())) {
                let mut pt = paint(Color::WHITE);
                pt.shader = sh;
                px.fill_path(&p, &pt, FillRule::Winding, ts, clip);
            }
            fill_o(px, star(-44.0, -44.0, 12.0, 0.3, 4), cream);
            fill_o(px, star(46.0, 40.0, 9.0, 0.3, 4), cream);
            text(px, "STAY RAD", &black, if v == 3 { cream } else { ink }, (-22.0, -5.0, 44.0, 10.0));
        }
        // Butterfly Bearings: a bearing on each wing.
        20 => {
            let (top, low, word) = [(c(240, 150, 190), c(250, 200, 120), ink), (c(130, 100, 200), c(120, 210, 220), ink), (c(250, 170, 60), c(240, 110, 60), ink), (c(150, 210, 170), c(240, 200, 230), ink)][v];
            fill(px, &pk[0], top);
            fill(px, &pk[1], top);
            fill(px, &pk[2], low);
            fill(px, &pk[3], low);
            for (x, y) in [(-40.0, -26.0), (40.0, -26.0)] {
                fill(px, &ellipse(x - 13.0, y - 13.0, 26.0, 26.0), c(200, 204, 212));
                fill(px, &ellipse(x - 6.0, y - 6.0, 12.0, 12.0), ink);
            }
            fill(px, &pk[4], ink);
            line(px, &[(-2.0, -40.0), (-12.0, -60.0)], 2.0, ink);
            line(px, &[(2.0, -40.0), (12.0, -60.0)], 2.0, ink);
            fill(px, &pk[5], ink);
            text(px, "BUTTERFLY BEARINGS", &bold, cream, (-46.0, 50.0, 92.0, 11.0));
            let _ = word;
        }
        // Kiss My Grip.
        21 => {
            let (bg, lips, word) = [(cream, c(200, 30, 46), ink), (ink, c(236, 80, 150), cream), (c(250, 200, 220), c(170, 20, 60), ink), (c(200, 180, 240), c(110, 40, 110), ink)][v];
            fill_o(px, inner(21, 0.9), bg);
            let mut pb = PathBuilder::new();
            pb.move_to(-54.0, 0.0);
            pb.cubic_to(-38.0, -28.0, -14.0, -30.0, 0.0, -16.0);
            pb.cubic_to(14.0, -30.0, 38.0, -28.0, 54.0, 0.0);
            pb.quad_to(0.0, 6.0, -54.0, 0.0);
            pb.close();
            fill_o(px, pb.finish(), lips);
            let mut pb = PathBuilder::new();
            pb.move_to(-54.0, 2.0);
            pb.quad_to(0.0, 10.0, 54.0, 2.0);
            pb.cubic_to(38.0, 34.0, -38.0, 34.0, -54.0, 2.0);
            pb.close();
            fill_o(px, pb.finish(), lips);
            fill(px, &ellipse(-14.0, 10.0, 18.0, 6.0), c(255, 230, 230));
            text(px, "KISS MY GRIP", &black, word, (-34.0, 26.0, 68.0, 10.0));
        }
        // After the sesh: a rainbow over a mini ramp.
        22 => {
            let bands = [
                [c(230, 70, 70), c(250, 160, 50), c(250, 220, 70), c(90, 180, 110), c(80, 140, 220)],
                [c(250, 180, 200), c(250, 210, 170), c(250, 240, 180), c(190, 230, 200), c(190, 200, 250)],
                [c(170, 30, 90), c(230, 80, 150), c(250, 150, 190), c(250, 200, 220), c(255, 236, 240)],
                [c(110, 60, 160), c(160, 110, 210), c(200, 160, 240), c(150, 220, 230), c(240, 240, 250)],
            ][v];
            for (i, col) in bands.iter().enumerate() {
                let r1 = 74.0 - i as f32 * 10.0;
                fill_o(px, band(r1 - 10.0, r1), *col);
            }
            fill_o(px, poly(&[(-40.0, 30.0), (-40.0, 10.0), (-34.0, 22.0), (-24.0, 28.0), (24.0, 28.0), (34.0, 22.0), (40.0, 10.0), (40.0, 30.0)], true), ink);
            text(px, "AFTER THE SESH", &black, ink, (-50.0, 32.0, 100.0, 12.0));
        }
        // Purr Skateboards: a cat.
        23 => {
            let (fur, inside, word) = [(cream, c(250, 170, 190), ink), (ink, c(236, 80, 150), cream), (c(250, 180, 120), c(250, 200, 200), ink), (c(170, 160, 200), c(250, 210, 230), ink)][v];
            for p in &pk { fill(px, p, fur); }
            fill_o(px, poly(&[(-50.0, -22.0), (-44.0, -58.0), (-22.0, -38.0)], true), inside);
            fill_o(px, poly(&[(50.0, -22.0), (44.0, -58.0), (22.0, -38.0)], true), inside);
            fill(px, &ellipse(-30.0, -14.0, 14.0, 18.0), word);
            fill(px, &ellipse(16.0, -14.0, 14.0, 18.0), word);
            fill_o(px, poly(&[(-6.0, 6.0), (6.0, 6.0), (0.0, 13.0)], true), inside);
            for (y, d) in [(8.0, -3.0), (14.0, 2.0)] {
                line(px, &[(-20.0, y), (-50.0, y + d)], 1.6, word);
                line(px, &[(20.0, y), (50.0, y + d)], 1.6, word);
            }
            text(px, "PURR", &black, word, (-28.0, 20.0, 56.0, 18.0));
            text(px, "SKATEBOARDS", &bold, word, (-26.0, 40.0, 52.0, 8.0));
        }
        // Flower Powerslide.
        24 => {
            let (petal, centre, word) = [(cream, c(250, 200, 60), ink), (c(250, 170, 200), c(250, 220, 90), ink), (c(200, 170, 240), cream, ink), (c(160, 210, 240), c(250, 150, 60), ink)][v];
            for p in &pk[..10] { fill(px, p, petal); }
            fill(px, &pk[10], centre);
            text(px, "FLOWER", &black, word, (-22.0, -14.0, 44.0, 10.0));
            text(px, "POWERSLIDE", &bold, word, (-24.0, 0.0, 48.0, 9.0));
        }
        // Berry Gnarly.
        25 => {
            let (berry, word) = [(c(220, 40, 60), cream), (c(240, 110, 160), cream), (c(250, 190, 210), ink), (ink, c(240, 110, 160))][v];
            fill(px, &pk[0], berry);
            for p in &pk[1..] { fill(px, p, c(80, 160, 80)); }
            for (x, y) in [(-30.0, -18.0), (24.0, -20.0), (-36.0, 8.0), (34.0, 6.0), (-14.0, 26.0), (14.0, 28.0), (0.0, 46.0), (-20.0, 44.0), (20.0, 44.0)] {
                fill_o(px, rot(ellipse(x - 2.0, y - 3.0, 4.0, 6.0), 20.0, x, y), c(250, 236, 160));
            }
            text(px, "BERRY", &black, word, (-30.0, -14.0, 60.0, 14.0));
            text(px, "GNARLY", &black, word, (-26.0, 4.0, 52.0, 12.0));
        }
        // Night sesh: moon and stars.
        26 => {
            let (sky, moon) = [(c(30, 36, 80), c(250, 230, 160)), (c(80, 50, 120), c(250, 210, 230)), (ink, c(220, 220, 235)), (c(40, 90, 110), c(250, 220, 120))][v];
            fill_o(px, inner(26, 0.9), sky);
            fill(px, &ellipse(-36.0, -44.0, 64.0, 64.0), moon);
            fill(px, &ellipse(-18.0, -52.0, 60.0, 60.0), sky);
            for (x, y, r) in [(26.0, -30.0, 7.0), (38.0, 0.0, 5.0), (12.0, 10.0, 4.0), (-44.0, 14.0, 4.0)] { fill_o(px, star(x, y, r, 0.45, 5), moon); }
            text(px, "NIGHT SESH", &black, moon, (-34.0, 26.0, 68.0, 12.0));
        }
        // A skater, the filmer's counterpart: long hair, beanie, board under her arm.
        27 => {
            fill(px, &rect(-90.0, -90.0, 180.0, 180.0), Color::from_rgba8(255, 255, 255, 70));
            let line_c = Color::from_rgba8(84, 58, 160, 255);
            let (hair, beanie, top, pants, board) = [
                (c(120, 70, 40), c(220, 70, 90), c(250, 236, 200), c(120, 150, 200), c(232, 196, 64)),
                (c(30, 26, 30), c(250, 200, 60), c(240, 140, 180), c(80, 80, 90), c(130, 200, 190)),
                (c(230, 190, 110), c(120, 200, 170), c(200, 160, 230), c(196, 176, 130), c(240, 110, 80)),
                (c(170, 60, 50), c(60, 70, 120), c(150, 196, 80), c(60, 60, 70), c(250, 214, 80)),
            ][v];
            let off = |pts: &[(f32, f32)]| -> Vec<(f32, f32)> { pts.iter().map(|p| (p.0 + 2.5, p.1 + 1.5)).collect() };
            let closed = |px: &mut Pixmap, col: Color, pts: &[(f32, f32)]| if let Some(p) = super::curve(&off(pts), 0.4, true) { px.fill_path(&p, &paint(col), FillRule::EvenOdd, ts, clip); };
            let pen = tiny_skia::Stroke { width: 2.4, line_join: tiny_skia::LineJoin::Round, ..Default::default() };
            closed(px, hair, SK_HAIR);
            let mut hp = SK_HAIR.to_vec();
            hp.push(SK_HAIR[0]);
            super::wobble(px, ts, clip, &pen, line_c, rnd, &hp);
            fill(px, &round_rect(32.5, -42.5, 16.0, 104.0, 8.0), board);
            closed(px, top, SK_TOP);
            closed(px, pants, SK_PANTS);
            closed(px, c(250, 232, 214), SK_HEAD);
            closed(px, beanie, SK_BEANIE);
            closed(px, c(250, 232, 214), SK_ARM);
            for (x, y) in [(-32.0, 62.0), (4.0, 62.0)] { fill(px, &ellipse(x, y, 26.0, 12.0), cream); }
            for part in [SK_HEAD, SK_BEANIE, SK_TOP, SK_PANTS, SK_ARM] {
                let mut pts = part.to_vec();
                pts.push(part[0]);
                super::wobble(px, ts, clip, &pen, line_c, rnd, &pts);
            }
            px.stroke_path(&round_rect(30.0, -44.0, 16.0, 104.0, 8.0), &paint(line_c), &pen, ts, clip);
            for (x, y) in [(26.0, -34.0), (46.0, -34.0), (26.0, 48.0), (46.0, 48.0)] {
                fill(px, &ellipse(x - 4.0, y, 8.0, 10.0), cream);
                px.stroke_path(&ellipse(x - 4.0, y, 8.0, 10.0), &paint(line_c), &pen, ts, clip);
            }
            for (x, y) in [(-32.0, 62.0), (4.0, 62.0)] { px.stroke_path(&ellipse(x, y, 26.0, 12.0), &paint(line_c), &pen, ts, clip); }
            super::wobble(px, ts, clip, &pen, line_c, rnd, &[(-5.0, -46.0), (-3.0, -45.0)]);
            super::wobble(px, ts, clip, &pen, line_c, rnd, &[(4.0, -46.0), (6.0, -45.0)]);
            super::wobble(px, ts, clip, &pen, line_c, rnd, &[(-3.0, -39.0), (1.0, -37.0), (4.0, -39.0)]);
            if let Some(sh) = cut(27) { px.stroke_path(&sh, &paint(Color::from_rgba8(255, 255, 255, 130)), &round_stroke(1.5), ts, clip); }
        }
        // Have a rad day: a star-eyed smiley.
        28 => {
            let (face, word) = [(c(250, 214, 60), ink), (c(250, 170, 200), ink), (c(150, 220, 200), ink), (ink, c(250, 214, 60))][v];
            fill_o(px, inner(28, 0.9), face);
            fill_o(px, star(-20.0, -16.0, 12.0, 0.45, 5), word);
            fill_o(px, star(20.0, -16.0, 12.0, 0.45, 5), word);
            line(px, &ring_pts(0.0, 0.0, 30.0, 24.0, 20.0, 160.0, 20), 4.0, word);
            text(px, "HAVE A RAD DAY", &black, word, (-34.0, 34.0, 68.0, 9.0));
        }
        // Soft Serve Skates.
        29 => {
            let (cream_c, cone, word) = [(c(250, 200, 220), c(220, 170, 100), ink), (c(170, 220, 190), c(220, 170, 100), ink), (c(200, 170, 240), c(200, 150, 90), ink), (c(250, 240, 220), c(150, 100, 60), c(200, 30, 60))][v];
            fill(px, &pk[0], cone);
            for k in 0..5 { let x = -30.0 + k as f32 * 14.0; line(px, &[(x, 10.0), (x + 22.0, 46.0)], 1.4, c(160, 110, 60)); }
            fill(px, &pk[1], cream_c);
            for p in &pk[2..5] { fill(px, p, cream_c); }
            fill(px, &pk[5], c(200, 30, 46));
            text(px, "SOFT SERVE", &black, word, (-38.0, -34.0, 76.0, 14.0));
            text(px, "SKATES", &hand, word, (-22.0, -16.0, 44.0, 14.0));
        }
        _ => {}
    }
}
