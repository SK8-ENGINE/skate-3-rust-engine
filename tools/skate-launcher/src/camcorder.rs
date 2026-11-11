//! The camcorder LCD panel look of the launcher window (docs/hails-additions/41-launcher-camcorder-viewfinder.md; the
//! measurements: 44a section 6): the flip-out screen of a mid-2000s HD camcorder, drawn in a 1240 x 760 unit space
//! scaled into the window. The window's logic (ui.rs) hands over what to show; this draws it and reports clicks.
use eframe::egui::{self, pos2, vec2, Color32, CornerRadius, FontId, Pos2, Rect, Shape, Stroke, StrokeKind};

pub const UNIT: egui::Vec2 = vec2(1240.0, 760.0);
const PANEL: Rect = Rect { min: pos2(20.0, 20.0), max: pos2(1192.0, 740.0) };
const SCREEN: Rect = Rect { min: pos2(172.0, 52.0), max: pos2(1162.0, 609.0) };
const HINGE: Rect = Rect { min: pos2(1186.0, 150.0), max: pos2(1224.0, 610.0) };

const PLASTIC: Color32 = Color32::from_rgb(54, 56, 62);
const PLASTIC_HI: Color32 = Color32::from_rgb(100, 103, 112);
const PLASTIC_DARK: Color32 = Color32::from_rgb(24, 25, 29);
const PRINT: Color32 = Color32::from_rgb(200, 202, 208);
const REC_RED: Color32 = Color32::from_rgb(230, 40, 32);
const INK: Color32 = Color32::from_rgb(244, 246, 250);
const DIM: Color32 = Color32::from_rgb(168, 178, 196);
const AMBER_HI: Color32 = Color32::from_rgb(255, 198, 88);
const AMBER_LO: Color32 = Color32::from_rgb(238, 124, 12);
const AMBER: Color32 = Color32::from_rgb(255, 172, 44);
const GREEN: Color32 = Color32::from_rgb(124, 220, 96);
const RED: Color32 = Color32::from_rgb(255, 98, 86);

/// The bezel keys: HOME = back, SET = select, POWER = close.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Key {
    Home,
    Set,
    Power,
}

const KEYS: &[(Key, &str, Rect)] = &[
    (Key::Home, "HOME", Rect { min: pos2(40.0, 70.0), max: pos2(150.0, 118.0) }),
    (Key::Set, "SET", Rect { min: pos2(40.0, 372.0), max: pos2(150.0, 420.0) }),
    (Key::Power, "POWER", Rect { min: pos2(48.0, 588.0), max: pos2(142.0, 624.0) }),
];

/// What the LCD shows this frame.
pub struct View<'a> {
    pub title: &'a str,
    pub items: Vec<(&'a str, bool, bool)>,
    pub sel: usize,
    pub body: &'a [String],
    pub mono: bool,
    pub hint: &'a str,
    pub ready: Option<bool>,
    pub info: &'a str,
    pub records: &'a [String],
    pub job: Option<JobView<'a>>,
    pub recording: bool,
    pub seconds: f32,
}

pub struct JobView<'a> {
    pub title: &'a str,
    pub progress: Option<(u32, u32)>,
    pub krate: &'a str,
    pub compiled: u32,
    pub warnings: u32,
    pub waiting: Option<&'a str>,
    pub stage: &'a str,
    pub lines: Vec<&'a str>,
    pub elapsed: f32,
}

/// What the frame's input hit.
#[derive(Default)]
pub struct Hits {
    pub item: Option<usize>,
    pub key: Option<Key>,
    pub drag: bool,
}

fn lerp(a: Color32, b: Color32, t: f32) -> Color32 {
    let t = t.clamp(0.0, 1.0);
    let m = |x: u8, y: u8| (x as f32 + (y as f32 - x as f32) * t).round() as u8;
    Color32::from_rgba_unmultiplied(m(a.r(), b.r()), m(a.g(), b.g()), m(a.b(), b.b()), m(a.a(), b.a()))
}

fn alpha(c: Color32, a: f32) -> Color32 {
    Color32::from_rgba_unmultiplied(c.r(), c.g(), c.b(), (a.clamp(0.0, 1.0) * 255.0) as u8)
}

/// A vertical gradient filling a rect (stops at 0..1).
fn vgrad(rect: Rect, stops: &[(f32, Color32)]) -> Shape {
    let mut mesh = egui::Mesh::default();
    for (i, (t, c)) in stops.iter().enumerate() {
        let y = rect.top() + rect.height() * t;
        mesh.colored_vertex(pos2(rect.left(), y), *c);
        mesh.colored_vertex(pos2(rect.right(), y), *c);
        if i > 0 {
            let b = (i as u32) * 2;
            mesh.add_triangle(b - 2, b - 1, b);
            mesh.add_triangle(b - 1, b + 1, b);
        }
    }
    Shape::mesh(mesh)
}

/// A horizontal gradient (stops at 0..1).
fn hgrad(rect: Rect, stops: &[(f32, Color32)]) -> Shape {
    let mut mesh = egui::Mesh::default();
    for (i, (t, c)) in stops.iter().enumerate() {
        let x = rect.left() + rect.width() * t;
        mesh.colored_vertex(pos2(x, rect.top()), *c);
        mesh.colored_vertex(pos2(x, rect.bottom()), *c);
        if i > 0 {
            let b = (i as u32) * 2;
            mesh.add_triangle(b - 2, b - 1, b);
            mesh.add_triangle(b - 1, b + 1, b);
        }
    }
    Shape::mesh(mesh)
}

/// Maps unit space into the window: uniform scale, centred, and the panel's swing (0 = shut against the hinge, 1 =
/// open) as a horizontal squash towards the hinge.
pub struct Space {
    origin: Pos2,
    scale: f32,
    swing: f32,
}

impl Space {
    pub fn new(window: Rect, swing: f32) -> Space {
        let scale = (window.width() / UNIT.x).min(window.height() / UNIT.y);
        let origin = window.center() - UNIT * scale / 2.0;
        Space { origin, scale, swing: swing.clamp(0.0, 1.2) }
    }
    fn p(&self, u: Pos2) -> Pos2 {
        let hinge_x = HINGE.center().x;
        let x = hinge_x + (u.x - hinge_x) * self.swing;
        self.origin + vec2(x, u.y) * self.scale
    }
    pub fn r(&self, u: Rect) -> Rect {
        Rect::from_two_pos(self.p(u.min), self.p(u.max))
    }
    fn s(&self, v: f32) -> f32 {
        v * self.scale
    }
}

fn text(painter: &egui::Painter, sp: &Space, at: Pos2, anchor: egui::Align2, s: &str, size: f32, color: Color32, mono: bool) -> Rect {
    let font = if mono { FontId::monospace(sp.s(size)) } else { FontId::proportional(sp.s(size)) };
    painter.text(sp.p(at), anchor, s, font, color)
}

/// One key on the bezel: a gradient face, a rim, its label.
fn hard_key(painter: &egui::Painter, sp: &Space, rect: Rect, label: &str, hover: bool) {
    let r = sp.r(rect);
    let (top, bottom) = if hover { (Color32::from_rgb(132, 136, 146), Color32::from_rgb(70, 72, 80)) } else { (Color32::from_rgb(92, 95, 104), Color32::from_rgb(44, 46, 52)) };
    painter.rect_filled(r.expand(sp.s(3.0)), CornerRadius::same(sp.s(12.0) as u8), PLASTIC_DARK);
    painter.add(vgrad(r, &[(0.0, top), (1.0, bottom)]));
    painter.rect_stroke(r, CornerRadius::same(sp.s(10.0) as u8), Stroke::new(sp.s(1.5), alpha(Color32::BLACK, 0.6)), StrokeKind::Outside);
    text(painter, sp, rect.center(), egui::Align2::CENTER_CENTER, label, 19.0, PRINT, false);
}

/// The whole panel; returns what the pointer hit.
pub fn paint(ui: &mut egui::Ui, view: &View, swing: f32, screen_level: f32) -> Hits {
    let window = ui.max_rect();
    let sp = Space::new(window, swing);
    let painter = ui.painter().clone();
    let mut hits = Hits::default();
    let pointer = ui.ctx().input(|i| i.pointer.hover_pos());
    let clicked = ui.ctx().input(|i| i.pointer.primary_clicked());
    let pressed = ui.ctx().input(|i| i.pointer.primary_pressed());

    // Hinge barrel (stays put while the panel swings).
    let hinge = sp.r(HINGE);
    painter.add(hgrad(hinge, &[(0.0, PLASTIC_DARK), (0.28, PLASTIC_HI), (0.62, PLASTIC), (1.0, PLASTIC_DARK)]));
    for y in (200..600).step_by(70) {
        let a = sp.p(pos2(HINGE.left() + 4.0, y as f32));
        let b = sp.p(pos2(HINGE.right() - 4.0, y as f32));
        painter.line_segment([a, b], Stroke::new(sp.s(1.5), alpha(Color32::BLACK, 0.5)));
    }

    // Panel body.
    let body = sp.r(PANEL);
    let radius = CornerRadius::same(sp.s(34.0).min(120.0) as u8);
    painter.rect_filled(body.expand(sp.s(3.0)), radius, alpha(Color32::BLACK, 0.82));
    painter.add(Shape::rect_filled(body, radius, PLASTIC));
    painter.add(vgrad(body.shrink(sp.s(18.0)), &[(0.0, Color32::from_rgb(76, 78, 86)), (1.0, Color32::from_rgb(32, 33, 38))]));
    painter.rect_stroke(body.shrink(sp.s(10.0)), CornerRadius::same(sp.s(26.0).min(120.0) as u8), Stroke::new(sp.s(1.0), alpha(Color32::WHITE, 0.12)), StrokeKind::Inside);
    text(&painter, &sp, pos2(822.0, 700.0), egui::Align2::LEFT_CENTER, "SK8-1000  HD", 18.0, PRINT, false);
    painter.circle_filled(sp.p(pos2(1150.0, 36.0)), sp.s(6.0), PLASTIC_DARK);

    for (key, label, rect) in KEYS {
        let hover = pointer.is_some_and(|p| sp.r(*rect).contains(p));
        hard_key(&painter, &sp, *rect, label, hover);
        if hover && clicked {
            hits.key = Some(*key);
        }
    }

    // Gasket and screen.
    let screen = sp.r(SCREEN);
    painter.rect_filled(screen.expand(sp.s(12.0)), CornerRadius::same(sp.s(10.0) as u8), Color32::from_rgb(12, 12, 14));
    let lcd = |c: Color32| lerp(Color32::from_rgb(4, 5, 8), c, screen_level);
    painter.add(vgrad(screen, &[(0.0, lcd(Color32::from_rgb(28, 40, 74))), (0.55, lcd(Color32::from_rgb(58, 46, 78))), (0.8, lcd(Color32::from_rgb(110, 62, 58))), (1.0, lcd(Color32::from_rgb(30, 24, 30)))]));
    if screen_level > 0.05 {
        let ink = |c: Color32| lerp(Color32::from_rgb(4, 5, 8), c, screen_level);
        lcd_content(&painter, &sp, view, &ink, pointer, clicked, &mut hits);
    }
    // Glass shine.
    painter.add(vgrad(Rect::from_min_max(screen.min, pos2(screen.max.x, screen.min.y + screen.height() * 0.35)), &[(0.0, alpha(Color32::WHITE, 0.07)), (1.0, alpha(Color32::WHITE, 0.0))]));

    // Dragging the panel by its bezel (not the screen, not the keys) moves the window.
    if pressed && hits.key.is_none() && pointer.is_some_and(|p| body.contains(p) && !screen.contains(p)) {
        hits.drag = true;
    }
    hits
}

/// Everything on the LCD.
fn lcd_content(painter: &egui::Painter, sp: &Space, view: &View, ink: &dyn Fn(Color32) -> Color32, pointer: Option<Pos2>, clicked: bool, hits: &mut Hits) {
    let left = SCREEN.left() + 22.0;
    let right = SCREEN.right() - 22.0;
    // Status row: STBY / REC with its dot, the HDV badge, the tape counter, the battery.
    let top = SCREEN.top() + 26.0;
    if view.recording {
        let blink = (view.seconds * 2.0).fract() < 0.6;
        if blink {
            painter.circle_filled(sp.p(pos2(left + 8.0, top)), sp.s(7.0), ink(REC_RED));
        }
        text(painter, sp, pos2(left + 22.0, top), egui::Align2::LEFT_CENTER, "REC", 20.0, ink(REC_RED), false);
    } else {
        text(painter, sp, pos2(left, top), egui::Align2::LEFT_CENTER, "STBY", 20.0, ink(GREEN), false);
    }
    let badge = sp.r(Rect::from_min_size(pos2(left + 92.0, top - 12.0), vec2(96.0, 24.0)));
    painter.rect_stroke(badge, CornerRadius::same(sp.s(4.0) as u8), Stroke::new(sp.s(1.5), ink(DIM)), StrokeKind::Inside);
    text(painter, sp, pos2(left + 140.0, top), egui::Align2::CENTER_CENTER, "HDV 1080i", 15.0, ink(DIM), false);
    let t = if view.recording { view.seconds as u32 } else { 0 };
    text(painter, sp, pos2(right - 90.0, top), egui::Align2::RIGHT_CENTER, &format!("{:01}:{:02}:{:02}", t / 3600, t / 60 % 60, t % 60), 20.0, ink(INK), true);
    let battery = Rect::from_min_size(pos2(right - 66.0, top - 10.0), vec2(56.0, 20.0));
    painter.rect_stroke(sp.r(battery), CornerRadius::same(sp.s(3.0) as u8), Stroke::new(sp.s(1.5), ink(INK)), StrokeKind::Inside);
    painter.rect_filled(sp.r(Rect::from_min_size(pos2(battery.left() + 4.0, battery.top() + 4.0), vec2(40.0, 12.0))), 0, ink(GREEN));

    // Title tab.
    let tab_top = top + 26.0;
    let tab = Rect::from_min_size(pos2(left, tab_top), vec2(right - left, 34.0));
    painter.add(vgrad(sp.r(tab), &[(0.0, ink(Color32::from_rgb(50, 104, 196))), (1.0, ink(Color32::from_rgb(22, 52, 120)))]));
    text(painter, sp, pos2(left + 14.0, tab.center().y), egui::Align2::LEFT_CENTER, view.title, 21.0, ink(INK), false);

    let body_top = tab.bottom() + 12.0;
    let footer_y = SCREEN.bottom() - 22.0;
    if let Some(job) = &view.job {
        job_card(painter, sp, job, ink, left, right, body_top, footer_y);
    } else {
        // List on the left, info on the right.
        let split = left + (right - left) * 0.52;
        let mut y = body_top;
        for line in view.body.iter().take(12) {
            text(painter, sp, pos2(left + 8.0, y + 11.0), egui::Align2::LEFT_CENTER, line, if view.mono { 15.0 } else { 18.0 }, ink(INK), view.mono);
            y += 24.0;
        }
        if !view.body.is_empty() {
            y += 8.0;
        }
        let row_h = 36.0;
        let visible = ((footer_y - 30.0 - y) / row_h).floor().max(1.0) as usize;
        let first = view.sel.saturating_sub(visible.saturating_sub(1));
        for (i, (label, on, current)) in view.items.iter().enumerate().skip(first).take(visible) {
            let row = Rect::from_min_size(pos2(left, y), vec2(split - left - 12.0, row_h - 4.0));
            if i == view.sel {
                painter.add(vgrad(sp.r(row), &[(0.0, ink(AMBER_HI)), (0.5, ink(AMBER)), (1.0, ink(AMBER_LO))]));
            }
            let color = if !on { ink(lerp(DIM, Color32::BLACK, 0.35)) } else if i == view.sel { ink(Color32::from_rgb(30, 22, 10)) } else { ink(INK) };
            if *current {
                painter.circle_filled(sp.p(pos2(row.left() + 10.0, row.center().y)), sp.s(5.0), ink(AMBER));
            }
            let clipped = painter.with_clip_rect(sp.r(row).shrink2(vec2(0.0, -4.0)));
            text(&clipped, sp, pos2(row.left() + 24.0, row.center().y), egui::Align2::LEFT_CENTER, label, 20.0, color, false);
            if pointer.is_some_and(|p| sp.r(row).contains(p)) && clicked {
                hits.item = Some(i);
            }
            y += row_h;
        }
        if !view.items.is_empty() {
            let mut iy = body_top;
            text(painter, sp, pos2(split, iy + 10.0), egui::Align2::LEFT_CENTER, "NOW SELECTED", 14.0, ink(DIM), false);
            iy += 28.0;
            if let Some(ready) = view.ready {
                let (word, c) = if ready { ("READY", GREEN) } else { ("NOT AVAILABLE", RED) };
                let pill = Rect::from_min_size(pos2(split, iy), vec2(if ready { 84.0 } else { 150.0 }, 24.0));
                painter.rect_filled(sp.r(pill), CornerRadius::same(sp.s(12.0) as u8), ink(alpha(c, 0.25)));
                text(painter, sp, pill.center(), egui::Align2::CENTER_CENTER, word, 14.0, ink(c), false);
                iy += 34.0;
            }
            for chunk in wrap(view.hint, 42).iter().chain(std::iter::once(&String::new())).chain(wrap(view.info, 42).iter()) {
                if iy > footer_y - 40.0 { break; }
                text(painter, sp, pos2(split, iy + 9.0), egui::Align2::LEFT_CENTER, chunk, 16.0, ink(INK), false);
                iy += 22.0;
            }
            for r in view.records {
                if iy > footer_y - 40.0 { break; }
                match r.strip_prefix('#') {
                    Some(head) => text(painter, sp, pos2(split, iy + 9.0), egui::Align2::LEFT_CENTER, head, 15.0, ink(AMBER), false),
                    None => text(painter, sp, pos2(split + 12.0, iy + 9.0), egui::Align2::LEFT_CENTER, r, 14.0, ink(DIM), false),
                };
                iy += 20.0;
            }
        }
    }

    // Footer: the buttons and the date.
    let badge = |x: f32, k: &str, what: &str, c: Color32| {
        painter.circle_filled(sp.p(pos2(x, footer_y)), sp.s(11.0), ink(c));
        text(painter, sp, pos2(x, footer_y), egui::Align2::CENTER_CENTER, k, 14.0, ink(Color32::BLACK), false);
        text(painter, sp, pos2(x + 18.0, footer_y), egui::Align2::LEFT_CENTER, what, 16.0, ink(DIM), false);
    };
    if view.job.is_some() {
        badge(left + 12.0, "B", "twice to cancel", RED);
    } else {
        badge(left + 12.0, "A", "select", GREEN);
        badge(left + 130.0, "B", "back", RED);
    }
    let date = jiff::Zoned::now().strftime("%b %d %Y  %H:%M").to_string().to_uppercase();
    text(painter, sp, pos2(right, footer_y), egui::Align2::RIGHT_CENTER, &date, 16.0, ink(DIM), true);
}

#[allow(clippy::too_many_arguments)]
fn job_card(painter: &egui::Painter, sp: &Space, job: &JobView, ink: &dyn Fn(Color32) -> Color32, left: f32, right: f32, top: f32, bottom: f32) {
    let mut y = top + 12.0;
    text(painter, sp, pos2(left + 8.0, y), egui::Align2::LEFT_CENTER, job.title, 24.0, ink(INK), false);
    text(painter, sp, pos2(right, y), egui::Align2::RIGHT_CENTER, &format!("{:.0} s", job.elapsed), 18.0, ink(DIM), true);
    y += 34.0;
    if let Some(w) = job.waiting {
        let card = Rect::from_min_size(pos2(left, y), vec2(right - left, 64.0));
        painter.rect_filled(sp.r(card), CornerRadius::same(sp.s(8.0) as u8), ink(alpha(AMBER_LO, 0.25)));
        text(painter, sp, pos2(left + 14.0, y + 20.0), egui::Align2::LEFT_CENTER, "WAITING", 18.0, ink(AMBER), false);
        text(painter, sp, pos2(left + 14.0, y + 44.0), egui::Align2::LEFT_CENTER, w, 15.0, ink(INK), false);
        y += 76.0;
    }
    if !job.stage.is_empty() {
        text(painter, sp, pos2(left + 8.0, y), egui::Align2::LEFT_CENTER, job.stage, 17.0, ink(DIM), false);
        y += 28.0;
    }
    if let Some((a, b)) = job.progress {
        let bar = Rect::from_min_size(pos2(left, y), vec2(right - left, 22.0));
        painter.rect_filled(sp.r(bar), CornerRadius::same(sp.s(11.0) as u8), ink(Color32::from_rgb(20, 22, 30)));
        let fill = Rect::from_min_size(bar.min, vec2(bar.width() * (a as f32 / b.max(1) as f32).clamp(0.0, 1.0), bar.height()));
        painter.add(vgrad(sp.r(fill), &[(0.0, ink(AMBER_HI)), (1.0, ink(AMBER_LO))]));
        text(painter, sp, bar.center(), egui::Align2::CENTER_CENTER, &format!("{a} / {b}"), 15.0, ink(INK), true);
        y += 32.0;
        text(painter, sp, pos2(left + 8.0, y), egui::Align2::LEFT_CENTER, &format!("{}   {} compiled   {} warnings", job.krate, job.compiled, job.warnings), 15.0, ink(DIM), false);
        y += 26.0;
    }
    let lines = ((bottom - 30.0 - y) / 19.0).max(0.0) as usize;
    let start = job.lines.len().saturating_sub(lines);
    for l in &job.lines[start..] {
        text(painter, sp, pos2(left + 8.0, y + 8.0), egui::Align2::LEFT_CENTER, &l.chars().take(110).collect::<String>(), 13.0, ink(DIM), true);
        y += 19.0;
    }
}

fn wrap(s: &str, width: usize) -> Vec<String> {
    let mut out = Vec::new();
    let mut line = String::new();
    for word in s.split_whitespace() {
        if !line.is_empty() && line.len() + word.len() + 1 > width {
            out.push(std::mem::take(&mut line));
        }
        if !line.is_empty() {
            line.push(' ');
        }
        line.push_str(word);
    }
    if !line.is_empty() {
        out.push(line);
    }
    out
}

/// The power-on swing: 0..1 over the opening with a small overshoot, then 1.
pub fn swing(t: f32) -> f32 {
    let d = 0.7;
    if t >= d {
        return 1.0;
    }
    let u = t / d;
    let s = 1.4;
    let u1 = u - 1.0;
    1.0 + (s + 1.0) * u1 * u1 * u1 + s * u1 * u1
}

/// The screen turning on after the swing (with one short dip).
pub fn screen_level(t: f32) -> f32 {
    let on = 0.55;
    if t < on {
        return 0.0;
    }
    let u = ((t - on) / 0.35).clamp(0.0, 1.0);
    if (0.3..0.4).contains(&u) { 0.4 } else { u }
}
