//! Button prompt glyphs for the connected pad (retail CustomButton names, TU3 0x82204614).
//! Mapping, style and size: docs/hails-additions/31-retail-menus.md#prompt-glyphs
use crate::input::controller_kind::{ControllerKind, Family, PromptLabel, PromptStyle};

/// Glyph size in pixels (square RGBA8).
pub(crate) const SIZE: u32 = 64;
/// Ball radius as a share of the box (retail: diameter about 0.83 of the placeholder).
const RADIUS: f32 = 0.415;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Dir {
    Up,
    Down,
    Left,
    Right,
}

/// What a prompt shows.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Prompt {
    /// A face button drawn in the family's look: Xbox a coloured ball, PlayStation a dark ball with the coloured
    /// symbol, Nintendo a dark ball with a light letter.
    Face(PromptLabel, PromptStyle),
    /// A dark pill with the printed name.
    Pill(&'static str),
    Stick { right: bool, dir: Option<Dir> },
    DPad(Option<Dir>),
}

impl Prompt {
    /// Texture cache key.
    pub(crate) fn key(&self) -> String {
        format!("prompt-glyph/{self:?}")
    }
}

/// The retail button name in `_test` -> face position (South, East, West, North), Xbox 360 naming.
pub(crate) fn face_index(retail: &str) -> Option<usize> {
    Some(match retail {
        "A" => 0,
        "B" => 1,
        "X" => 2,
        "Y" => 3,
        _ => return None,
    })
}

fn direction(suffix: &str) -> Option<Option<Dir>> {
    Some(match suffix {
        "" => None,
        "_Up" => Some(Dir::Up),
        "_Down" => Some(Dir::Down),
        "_L" | "_Left" => Some(Dir::Left),
        "_R" | "_Right" => Some(Dir::Right),
        // The replay editor's camera prompts: up / down (height) and the arrows (lens); shown as the plain D-pad.
        "_UpDown" | "_Arrows" => None,
        _ => return None,
    })
}

/// A retail button name that has a prompt.
pub(crate) fn known(retail: &str) -> bool {
    resolve(retail, Family::Xbox360, PromptStyle::Xbox.labels()).is_some()
}

/// The prompt for a retail button name on a pad of `family` with SDL face `labels`.
pub(crate) fn resolve(retail: &str, family: Family, labels: [PromptLabel; 4]) -> Option<Prompt> {
    // `_hud` names the HUD-style art of the same button (the replay screen's Hide prompt asks for `L3_Click_hud`,
    // `EnableShowHide`); the prompt is the same.
    let retail = retail.strip_suffix("_hud").unwrap_or(retail);
    if let Some(i) = face_index(retail) {
        return Some(Prompt::Face(labels[i], family.prompt_style()));
    }
    if let Some(rest) = retail.strip_prefix("Stick_Left") {
        return Some(Prompt::Stick { right: false, dir: direction(rest)? });
    }
    if let Some(rest) = retail.strip_prefix("Stick_Right") {
        return Some(Prompt::Stick { right: true, dir: direction(rest)? });
    }
    if let Some(rest) = retail.strip_prefix("DPad") {
        return Some(Prompt::DPad(direction(rest)?));
    }
    let style = family.prompt_style();
    let ps = style == PromptStyle::Playstation;
    let nintendo = style == PromptStyle::Nintendo;
    Some(Prompt::Pill(match retail {
        "L_Bumper" if ps => "L1",
        "R_Bumper" if ps => "R1",
        "L_Trigger" if ps => "L2",
        "R_Trigger" if ps => "R2",
        "L_Bumper" if nintendo => "L",
        "R_Bumper" if nintendo => "R",
        "L_Trigger" if nintendo => "ZL",
        "R_Trigger" if nintendo => "ZR",
        "L_Bumper" => "LB",
        "R_Bumper" => "RB",
        "L_Trigger" => "LT",
        "R_Trigger" => "RT",
        "L3_Click" if ps => "L3",
        "R3_Click" if ps => "R3",
        "L3_Click" => "LS",
        "R3_Click" => "RS",
        "Start" => match family {
            Family::XboxOne | Family::XboxElite => "MENU",
            Family::Playstation4 | Family::Playstation5 => "OPTIONS",
            _ if nintendo => "+",
            _ => "START",
        },
        "Back" => match family {
            Family::XboxOne | Family::XboxElite => "VIEW",
            Family::Playstation3 => "SELECT",
            Family::Playstation4 => "SHARE",
            Family::Playstation5 => "CREATE",
            _ if nintendo => "-",
            _ => "BACK",
        },
        _ => return None,
    }))
}

/// The prompt for the first connected pad (Xbox 360 naming without one).
pub(crate) fn resolve_for(retail: &str, pad: Option<&ControllerKind>) -> Option<Prompt> {
    match pad {
        Some(k) => resolve(retail, k.family, k.face_labels),
        None => resolve(retail, Family::Xbox360, PromptStyle::Xbox.labels()),
    }
}

/// The glyph as RGBA8, `SIZE` x `SIZE`, sRGB.
pub(crate) fn render(prompt: &Prompt) -> Vec<u8> {
    match prompt {
        Prompt::Face(label, style) => face(*label, *style),
        Prompt::Pill(text) => pill(text),
        Prompt::Stick { right, dir } => stick(*right, *dir),
        Prompt::DPad(dir) => dpad(*dir),
    }
}

// ---------------------------------------------------------------- strokes

enum Stroke {
    Line([f32; 2], [f32; 2]),
    /// Centre, radius, angle range in degrees (y down: 0 right, 90 down, -90 up), increasing from the first.
    Arc([f32; 2], f32, f32, f32),
}

fn distance(p: [f32; 2], stroke: &Stroke) -> f32 {
    match *stroke {
        Stroke::Line(a, b) => {
            let (ab, ap) = ([b[0] - a[0], b[1] - a[1]], [p[0] - a[0], p[1] - a[1]]);
            let t = ((ap[0] * ab[0] + ap[1] * ab[1]) / (ab[0] * ab[0] + ab[1] * ab[1])).clamp(0.0, 1.0);
            (ap[0] - ab[0] * t).hypot(ap[1] - ab[1] * t)
        }
        Stroke::Arc(c, r, a0, a1) => {
            let (dx, dy) = (p[0] - c[0], p[1] - c[1]);
            let angle = dy.atan2(dx).to_degrees();
            if (angle - a0).rem_euclid(360.0) <= a1 - a0 {
                (dx.hypot(dy) - r).abs()
            } else {
                let end = |a: f32| {
                    let (s, co) = a.to_radians().sin_cos();
                    (p[0] - c[0] - r * co).hypot(p[1] - c[1] - r * s)
                };
                end(a0).min(end(a1))
            }
        }
    }
}

/// Strokes of a character in a unit cell (y down).
fn character(c: char) -> Vec<Stroke> {
    use Stroke::{Arc, Line};
    let p_bowl = || {
        vec![Line([0.2, 0.0], [0.2, 1.0]), Line([0.2, 0.0], [0.5, 0.0]), Arc([0.5, 0.25], 0.25, -90.0, 90.0), Line([0.5, 0.5], [0.2, 0.5])]
    };
    match c {
        'A' => vec![Line([0.1, 1.0], [0.5, 0.0]), Line([0.5, 0.0], [0.9, 1.0]), Line([0.25, 0.62], [0.75, 0.62])],
        'B' => vec![
            Line([0.2, 0.0], [0.2, 1.0]),
            Line([0.2, 0.0], [0.5, 0.0]),
            Arc([0.5, 0.24], 0.24, -90.0, 90.0),
            Line([0.2, 0.48], [0.52, 0.48]),
            Arc([0.52, 0.74], 0.26, -90.0, 90.0),
            Line([0.2, 1.0], [0.52, 1.0]),
        ],
        'C' => vec![Arc([0.55, 0.5], 0.45, 45.0, 315.0)],
        'E' => vec![Line([0.2, 0.0], [0.2, 1.0]), Line([0.2, 0.0], [0.85, 0.0]), Line([0.2, 0.5], [0.7, 0.5]), Line([0.2, 1.0], [0.85, 1.0])],
        'H' => vec![Line([0.15, 0.0], [0.15, 1.0]), Line([0.85, 0.0], [0.85, 1.0]), Line([0.15, 0.5], [0.85, 0.5])],
        'I' => vec![Line([0.5, 0.0], [0.5, 1.0]), Line([0.25, 0.0], [0.75, 0.0]), Line([0.25, 1.0], [0.75, 1.0])],
        'K' => vec![Line([0.2, 0.0], [0.2, 1.0]), Line([0.85, 0.0], [0.2, 0.6]), Line([0.4, 0.42], [0.85, 1.0])],
        'L' => vec![Line([0.2, 0.0], [0.2, 1.0]), Line([0.2, 1.0], [0.8, 1.0])],
        'M' => vec![Line([0.1, 1.0], [0.1, 0.0]), Line([0.1, 0.0], [0.5, 0.6]), Line([0.5, 0.6], [0.9, 0.0]), Line([0.9, 0.0], [0.9, 1.0])],
        'N' => vec![Line([0.15, 1.0], [0.15, 0.0]), Line([0.15, 0.0], [0.85, 1.0]), Line([0.85, 1.0], [0.85, 0.0])],
        'O' => vec![Arc([0.5, 0.5], 0.45, 0.0, 360.0)],
        'P' => p_bowl(),
        'R' => {
            let mut s = p_bowl();
            s.push(Line([0.45, 0.5], [0.85, 1.0]));
            s
        }
        'S' => vec![Arc([0.5, 0.27], 0.27, 90.0, 340.0), Arc([0.5, 0.73], 0.27, -90.0, 160.0)],
        'T' => vec![Line([0.1, 0.0], [0.9, 0.0]), Line([0.5, 0.0], [0.5, 1.0])],
        'U' => vec![Line([0.15, 0.0], [0.15, 0.62]), Arc([0.5, 0.62], 0.35, 0.0, 180.0), Line([0.85, 0.62], [0.85, 0.0])],
        'V' => vec![Line([0.1, 0.0], [0.5, 1.0]), Line([0.5, 1.0], [0.9, 0.0])],
        'W' => vec![Line([0.05, 0.0], [0.27, 1.0]), Line([0.27, 1.0], [0.5, 0.35]), Line([0.5, 0.35], [0.73, 1.0]), Line([0.73, 1.0], [0.95, 0.0])],
        'Z' => vec![Line([0.15, 0.0], [0.85, 0.0]), Line([0.85, 0.0], [0.15, 1.0]), Line([0.15, 1.0], [0.85, 1.0])],
        '1' => vec![Line([0.3, 0.2], [0.55, 0.0]), Line([0.55, 0.0], [0.55, 1.0])],
        '2' => vec![Arc([0.5, 0.28], 0.28, 180.0, 380.0), Line([0.75, 0.42], [0.2, 1.0]), Line([0.2, 1.0], [0.85, 1.0])],
        '3' => vec![Arc([0.5, 0.27], 0.27, 200.0, 450.0), Arc([0.5, 0.73], 0.27, -90.0, 160.0)],
        '+' => vec![Line([0.5, 0.1], [0.5, 0.9]), Line([0.1, 0.5], [0.9, 0.5])],
        '-' => vec![Line([0.1, 0.5], [0.9, 0.5])],
        _ => Vec::new(),
    }
}

fn coverage(d: f32) -> f32 {
    (0.5 - d * SIZE as f32).clamp(0.0, 1.0)
}

/// Coverage of `text` centred on `centre`, fitted into `max_w` x `max_h` (box units).
fn text_ink(p: [f32; 2], text: &str, centre: [f32; 2], max_w: f32, max_h: f32) -> f32 {
    let chars: Vec<char> = text.chars().collect();
    let n = chars.len().max(1) as f32;
    let gap_ratio = 0.3;
    let cell_w = (max_w / (n + (n - 1.0) * gap_ratio)).min(max_h / 1.45);
    let cell_h = (cell_w * 1.45).min(max_h);
    let gap = cell_w * gap_ratio;
    let total = n * cell_w + (n - 1.0) * gap;
    let half_width = (0.03 * (cell_w / 0.17)).clamp(0.014, 0.034);
    let scale = cell_w.min(cell_h);
    let mut ink: f32 = 0.0;
    for (i, c) in chars.iter().enumerate() {
        let x0 = centre[0] - total / 2.0 + i as f32 * (cell_w + gap);
        let q = [(p[0] - x0) / cell_w, (p[1] - (centre[1] - cell_h / 2.0)) / cell_h];
        for s in character(*c) {
            ink = ink.max(coverage(distance(q, &s) * scale - half_width));
        }
    }
    ink
}

/// Lit from the top, like a button.
fn shade(y: f32) -> f32 {
    1.0 + 0.18 * (0.5 - y).max(-0.3) / 0.5
}

const OUTLINE: [f32; 3] = [0.08, 0.08, 0.09];
const DARK: [f32; 3] = [0.22, 0.23, 0.25];
const INK: f32 = 0.97;

fn raster(mut pixel: impl FnMut([f32; 2]) -> ([f32; 3], f32)) -> Vec<u8> {
    let n = SIZE as f32;
    let mut out = Vec::with_capacity((SIZE * SIZE * 4) as usize);
    for y in 0..SIZE {
        for x in 0..SIZE {
            let (rgb, alpha) = pixel([(x as f32 + 0.5) / n, (y as f32 + 0.5) / n]);
            out.extend(rgb.map(|c| (c.clamp(0.0, 1.0) * 255.0).round() as u8));
            out.push((alpha.clamp(0.0, 1.0) * 255.0).round() as u8);
        }
    }
    out
}

/// A body colour inside an outline, lit from the top, with ink on top.
fn body(fill: [f32; 3], inner: f32, ink: f32, y: f32) -> [f32; 3] {
    std::array::from_fn(|i| {
        let b = OUTLINE[i] + (fill[i] * shade(y) - OUTLINE[i]) * inner;
        b + (INK - b) * ink
    })
}

// ---------------------------------------------------------------- glyphs

fn face_shape(label: PromptLabel) -> (Vec<Stroke>, [f32; 3]) {
    use Stroke::{Arc, Line};
    match label {
        PromptLabel::A => (
            vec![Line([0.31, 0.74], [0.5, 0.26]), Line([0.5, 0.26], [0.69, 0.74]), Line([0.38, 0.57], [0.62, 0.57])],
            [0.45, 0.78, 0.33],
        ),
        PromptLabel::B => (
            vec![
                Line([0.37, 0.26], [0.37, 0.74]),
                Line([0.37, 0.26], [0.52, 0.26]),
                Arc([0.52, 0.37], 0.11, -90.0, 90.0),
                Line([0.37, 0.48], [0.54, 0.48]),
                Arc([0.54, 0.61], 0.13, -90.0, 90.0),
                Line([0.37, 0.74], [0.54, 0.74]),
            ],
            [0.91, 0.33, 0.3],
        ),
        PromptLabel::X => (vec![Line([0.33, 0.27], [0.67, 0.73]), Line([0.67, 0.27], [0.33, 0.73])], [0.31, 0.56, 0.93]),
        PromptLabel::Y => (
            vec![Line([0.33, 0.27], [0.5, 0.5]), Line([0.67, 0.27], [0.5, 0.5]), Line([0.5, 0.5], [0.5, 0.74])],
            [0.96, 0.78, 0.27],
        ),
        PromptLabel::Cross => (vec![Line([0.33, 0.33], [0.67, 0.67]), Line([0.67, 0.33], [0.33, 0.67])], [0.52, 0.66, 0.92]),
        PromptLabel::Circle => (vec![Arc([0.5, 0.5], 0.19, 0.0, 360.0)], [0.92, 0.38, 0.38]),
        PromptLabel::Square => (
            vec![
                Line([0.33, 0.33], [0.67, 0.33]),
                Line([0.67, 0.33], [0.67, 0.67]),
                Line([0.67, 0.67], [0.33, 0.67]),
                Line([0.33, 0.67], [0.33, 0.33]),
            ],
            [0.86, 0.55, 0.78],
        ),
        PromptLabel::Triangle => (
            vec![Line([0.5, 0.29], [0.7, 0.66]), Line([0.7, 0.66], [0.3, 0.66]), Line([0.3, 0.66], [0.5, 0.29])],
            [0.3, 0.78, 0.66],
        ),
    }
}

/// An Xbox-style face button: a coloured ball with a light symbol (retail's look).
pub(crate) fn glyph(label: PromptLabel) -> Vec<u8> {
    face(label, PromptStyle::Xbox)
}

fn face(label: PromptLabel, style: PromptStyle) -> Vec<u8> {
    let (strokes, colour) = face_shape(label);
    // PlayStation: the colour is the symbol's; Nintendo: no colour.
    let (ball, ink_colour) = match style {
        PromptStyle::Xbox => (colour, [INK; 3]),
        PromptStyle::Playstation => (DARK, colour.map(|c| (c * 1.15).min(1.0))),
        PromptStyle::Nintendo => (DARK, [INK; 3]),
    };
    raster(|p| {
        let r = (p[0] - 0.5).hypot(p[1] - 0.5);
        let disc = coverage(r - RADIUS);
        let inner = coverage(r - (RADIUS - 0.045));
        // The symbol at the ball's scale (strokes are authored for a 0.46 ball).
        let k = RADIUS / 0.46;
        let q = [0.5 + (p[0] - 0.5) / k, 0.5 + (p[1] - 0.5) / k];
        let ink = strokes.iter().map(|s| coverage((distance(q, s) - 0.045) * k)).fold(0.0, f32::max) * inner;
        let rgb: [f32; 3] = std::array::from_fn(|i| {
            let b = OUTLINE[i] + (ball[i] * shade(p[1]) - OUTLINE[i]) * inner;
            b + (ink_colour[i] - b) * ink
        });
        (rgb, disc)
    })
}

/// A named button: a dark pill with the light name, as wide as the face ball.
pub(crate) fn pill(text: &str) -> Vec<u8> {
    let (half_w, half_h) = (RADIUS + 0.04, 0.24);
    raster(|p| {
        let (dx, dy) = ((p[0] - 0.5).abs() - (half_w - half_h), (p[1] - 0.5).abs());
        let d = dx.max(0.0).hypot(dy) - half_h;
        let inner = coverage(d + 0.04);
        let ink = text_ink(p, text, [0.5, 0.5], 2.0 * (half_w - 0.07), 2.0 * (half_h - 0.09)) * inner;
        (body(DARK, inner, ink, p[1]), coverage(d))
    })
}

/// A stick: a dark ball with L / R and, for a direction, a light arrow at that edge.
fn stick(right: bool, dir: Option<Dir>) -> Vec<u8> {
    let letter = if right { "R" } else { "L" };
    raster(|p| {
        let r = (p[0] - 0.5).hypot(p[1] - 0.5);
        let inner = coverage(r - (RADIUS - 0.045));
        let ring = coverage((r - (RADIUS - 0.1)).abs() - 0.012) * 0.6;
        let mut ink = text_ink(p, letter, [0.5, 0.5], 0.2, 0.3).max(ring);
        if let Some(dir) = dir {
            // A triangle pointing out of that edge.
            let (ax, ay) = match dir {
                Dir::Up => (0.0, -1.0),
                Dir::Down => (0.0, 1.0),
                Dir::Left => (-1.0, 0.0),
                Dir::Right => (1.0, 0.0),
            };
            let (u, v) = ((p[0] - 0.5) * ax + (p[1] - 0.5) * ay, (p[0] - 0.5) * -ay + (p[1] - 0.5) * ax);
            let tip = RADIUS - 0.02;
            if u > tip - 0.13 && u < tip && v.abs() < (tip - u) * 0.75 {
                ink = 1.0;
            }
        }
        (body(DARK, inner, ink * inner, p[1]), coverage(r - RADIUS))
    })
}

/// The d-pad: a dark cross; the pressed arm lit (all arms half lit for plain "DPad").
fn dpad(dir: Option<Dir>) -> Vec<u8> {
    let (arm, half) = (RADIUS, 0.13);
    raster(|p| {
        let (x, y) = (p[0] - 0.5, p[1] - 0.5);
        let rect = |hx: f32, hy: f32| {
            let (qx, qy) = (x.abs() - hx, y.abs() - hy);
            qx.max(0.0).hypot(qy.max(0.0)) + qx.max(qy).min(0.0)
        };
        let cross = rect(arm, half).min(rect(half, arm)) - 0.02;
        let inner = coverage(cross + 0.04);
        let lit = match dir {
            None => 0.35,
            Some(dir) => {
                let on = match dir {
                    Dir::Up => y < -half && x.abs() < half,
                    Dir::Down => y > half && x.abs() < half,
                    Dir::Left => x < -half && y.abs() < half,
                    Dir::Right => x > half && y.abs() < half,
                };
                if on { 0.85 } else { 0.0 }
            }
        };
        (body(DARK, inner, lit * coverage(cross + 0.07), p[1]), coverage(cross))
    })
}

/// A UI image showing a pad button as the connected pad prints it (retail button name, Xbox 360 naming: "A",
/// "L_Bumper", "DPad_Left", ...). [`sync_ui_glyphs`] keeps its image current; an unknown name shows nothing.
#[derive(bevy::prelude::Component, Clone, Debug, PartialEq, Eq)]
pub(crate) struct PromptGlyph(pub &'static str);

/// Generated glyph images by prompt key, shared by every UI glyph.
#[derive(bevy::prelude::Resource, Default)]
pub(crate) struct PromptImages {
    images: std::collections::HashMap<String, bevy::prelude::Handle<bevy::prelude::Image>>,
    pad: Option<std::sync::Arc<ControllerKind>>,
}

impl PromptImages {
    /// The glyph image for `retail` on `pad` (generated once per prompt).
    pub(crate) fn get(
        &mut self,
        images: &mut bevy::prelude::Assets<bevy::prelude::Image>,
        retail: &str,
        pad: Option<&ControllerKind>,
    ) -> Option<bevy::prelude::Handle<bevy::prelude::Image>> {
        use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};
        let prompt = resolve_for(retail, pad)?;
        let handle = self.images.entry(prompt.key()).or_insert_with(|| {
            images.add(bevy::prelude::Image::new(
                Extent3d { width: SIZE, height: SIZE, depth_or_array_layers: 1 },
                TextureDimension::D2,
                render(&prompt),
                TextureFormat::Rgba8UnormSrgb,
                bevy::asset::RenderAssetUsages::RENDER_WORLD,
            ))
        });
        Some(handle.clone())
    }
}

/// The pad whose labels prompts show: the first connected one (Xbox naming without a pad), as the retail menus.
pub(crate) fn first_pad(controllers: &crate::input::ControllerInput) -> Option<std::sync::Arc<ControllerKind>> {
    controllers.kinds.iter().flatten().next().cloned()
}

/// Gives every [`PromptGlyph`] its image: when it is new or changed, and for all of them when the pad changes.
pub(crate) fn sync_ui_glyphs(
    controllers: Option<bevy::prelude::Res<crate::input::ControllerInput>>,
    mut cache: bevy::prelude::ResMut<PromptImages>,
    mut images: bevy::prelude::ResMut<bevy::prelude::Assets<bevy::prelude::Image>>,
    mut glyphs: bevy::prelude::Query<(bevy::prelude::Ref<PromptGlyph>, &mut bevy::prelude::ImageNode, &mut bevy::prelude::Node)>,
) {
    let pad = controllers.as_deref().and_then(first_pad);
    let pad_changed = pad.as_deref().map(|k| (k.family, k.face_labels)) != cache.pad.as_deref().map(|k| (k.family, k.face_labels));
    if pad_changed {
        cache.pad = pad.clone();
    }
    for (glyph, mut image, mut node) in &mut glyphs {
        if !pad_changed && !bevy::prelude::DetectChanges::is_changed(&glyph) {
            continue;
        }
        match cache.get(&mut images, glyph.0, pad.as_deref()) {
            Some(handle) => {
                if image.image != handle {
                    image.image = handle;
                }
            }
            None => node.display = bevy::prelude::Display::None,
        }
    }
}

/// A row of button hints: per item the pad's glyphs (retail button names), then the keyboard keys and the action in
/// text ("[A] Enter  Open"). Glyphs are sized to the text.
pub(crate) fn hint_row(
    parent: &mut bevy::ecs::hierarchy::ChildSpawnerCommands,
    items: &[(&[&'static str], &str, &str)],
    font_size: f32,
    color: bevy::prelude::Color,
) {
    use bevy::prelude::*;
    let side = Val::Px((font_size * 1.6).round());
    parent.spawn(Node { column_gap: Val::Px(font_size), align_items: AlignItems::Center, flex_wrap: FlexWrap::Wrap, ..default() }).with_children(|row| {
        for (buttons, keys, action) in items {
            row.spawn(Node { column_gap: Val::Px(4.), align_items: AlignItems::Center, ..default() }).with_children(|item| {
                for button in *buttons {
                    item.spawn((Node { width: side, height: side, ..default() }, ImageNode::default(), PromptGlyph(button)));
                }
                let keys = if keys.is_empty() { String::new() } else { format!("/ {keys}  ") };
                item.spawn((Text::new(format!("{keys}{action}")), TextFont { font_size, ..default() }, TextColor(color)));
            });
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn alpha(g: &[u8], x: f32, y: f32) -> u8 {
        g[(((y * SIZE as f32) as u32 * SIZE + (x * SIZE as f32) as u32) * 4 + 3) as usize]
    }
    fn rgb(g: &[u8], x: f32, y: f32) -> [u8; 3] {
        let i = (((y * SIZE as f32) as u32 * SIZE + (x * SIZE as f32) as u32) * 4) as usize;
        [g[i], g[i + 1], g[i + 2]]
    }

    #[test]
    fn faces_are_a_coloured_ball_with_a_light_symbol() {
        for label in [PromptLabel::A, PromptLabel::B, PromptLabel::Cross, PromptLabel::Circle, PromptLabel::Triangle] {
            let g = glyph(label);
            assert_eq!(g.len(), (SIZE * SIZE * 4) as usize);
            assert_eq!(alpha(&g, 0.0, 0.0), 0, "outside the ball is clear");
            assert_eq!(alpha(&g, 0.5, 0.07), 0, "the ball spans 0.83 of the box");
            assert!(alpha(&g, 0.5, 0.5) > 200, "the ball is opaque");
        }
        let a = glyph(PromptLabel::A);
        let ball = rgb(&a, 0.5, 0.82);
        assert!(ball[1] > ball[0] && ball[1] > ball[2], "green ball {ball:?}");
        assert!(rgb(&a, 0.5, 0.57).iter().all(|c| *c > 200), "light crossbar");
        let c = rgb(&glyph(PromptLabel::Circle), 0.5, 0.5);
        assert!(c[0] > c[1] && c[0] > c[2], "red ball {c:?}");
    }

    #[test]
    fn every_retail_button_has_an_equivalent_on_every_family() {
        let names = [
            "A", "B", "X", "Y", "Back", "Start", "R_Trigger", "L_Trigger", "L_Bumper", "R_Bumper", "L3_Click", "R3_Click",
            "Stick_Right_Up", "Stick_Right_Down", "Stick_Right_L", "Stick_Right_R", "Stick_Right", "DPad_Up", "DPad_Down",
            "DPad_Left", "DPad_Right", "Stick_Left_Up", "Stick_Left_Down", "Stick_Left_L", "Stick_Left_R", "Stick_Left", "DPad",
            // The HUD-style names (the replay screen's Hide prompt) and the camera edit prompts.
            "L3_Click_hud", "DPad_UpDown_hud", "DPad_Arrows_hud",
        ];
        let families = [
            Family::Xbox360,
            Family::XboxOne,
            Family::Playstation3,
            Family::Playstation4,
            Family::Playstation5,
            Family::SwitchPro,
            Family::Standard,
        ];
        for family in families {
            let labels = family.prompt_style().labels();
            for name in names {
                let prompt = resolve(name, family, labels).unwrap_or_else(|| panic!("{name} on {family:?}"));
                let g = render(&prompt);
                assert!(g.chunks(4).any(|px| px[3] > 0), "{name} on {family:?} draws");
                if let Prompt::Pill(text) = prompt {
                    assert!(text.chars().all(|c| !character(c).is_empty()), "font covers {text}");
                }
            }
        }
        let ps5 = Family::Playstation5;
        assert_eq!(resolve("R_Trigger", ps5, ps5.prompt_style().labels()), Some(Prompt::Pill("R2")));
        assert_eq!(resolve("Start", ps5, ps5.prompt_style().labels()), Some(Prompt::Pill("OPTIONS")));
        let switch = Family::SwitchPro;
        assert_eq!(resolve("L_Trigger", switch, switch.prompt_style().labels()), Some(Prompt::Pill("ZL")));
        assert_eq!(resolve("Back", switch, switch.prompt_style().labels()), Some(Prompt::Pill("-")));
        assert_eq!(resolve("A", switch, switch.prompt_style().labels()), Some(Prompt::Face(PromptLabel::B, PromptStyle::Nintendo)));
        // PlayStation: a dark ball with a coloured symbol.
        let circle = render(&resolve("B", ps5, ps5.prompt_style().labels()).unwrap());
        let (centre, ring) = (rgb(&circle, 0.5, 0.5), rgb(&circle, 0.5 + 0.19 * 0.415 / 0.46, 0.5));
        assert!(centre[0] < 90 && ring[0] > 180, "dark ball {centre:?}, red ring {ring:?}");
        assert_eq!(resolve("LB", Family::Xbox360, PromptStyle::Xbox.labels()), None);
        assert!(!known("Unknown"));
    }

    #[test]
    fn pill_and_dpad_shapes() {
        let lb = pill("LB");
        assert_eq!(alpha(&lb, 0.5, 0.1), 0, "the pill is shorter than the box");
        assert!(alpha(&lb, 0.5, 0.5) > 200 && alpha(&lb, 0.12, 0.5) > 200, "the pill spans the ball's width");
        let up = dpad(Some(Dir::Up));
        assert!(rgb(&up, 0.5, 0.2)[0] > rgb(&up, 0.5, 0.8)[0] + 60, "the up arm is lit");
        assert_eq!(alpha(&up, 0.15, 0.15), 0, "between the arms is clear");
    }

    /// Writes every prompt for every family as raw RGBA into `SKATE3_PROMPT_SHEET` (a folder) for a visual check:
    /// `cargo test -p skate-game prompt_sheet -- --ignored`.
    #[test]
    #[ignore = "writes files"]
    fn prompt_sheet() {
        let Some(dir) = std::env::var_os("SKATE3_PROMPT_SHEET") else { return };
        let names = ["A", "B", "X", "Y", "L_Bumper", "R_Bumper", "L_Trigger", "R_Trigger", "Start", "Back", "L3_Click",
            "R3_Click", "Stick_Left", "Stick_Right_Up", "DPad", "DPad_Left"];
        for family in [Family::Xbox360, Family::XboxOne, Family::Playstation4, Family::Playstation5, Family::SwitchPro] {
            let mut row = Vec::new();
            for name in names {
                row.push(render(&resolve(name, family, family.prompt_style().labels()).unwrap()));
            }
            let mut out = Vec::new();
            for y in 0..SIZE as usize {
                for g in &row {
                    out.extend_from_slice(&g[y * SIZE as usize * 4..(y + 1) * SIZE as usize * 4]);
                }
            }
            std::fs::write(std::path::Path::new(&dir).join(format!("{family:?}.rgba")), out).unwrap();
        }
    }
}
