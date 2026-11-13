//! The PC overlay's look and motion, ported from the recomp's settings overlay (rexglue-sdk); rows are ours.
//! Details: docs/hails-additions/31-retail-menus.md#pc-overlay-look
use super::{Kind, Menu, RowCtx, RowView};
use bevy::{
    asset::RenderAssetUsages,
    ecs::query::QueryData,
    prelude::*,
    render::render_resource::{Extent3d, TextureDimension, TextureFormat},
    text::LineBreak,
    window::PrimaryWindow,
};

/// The menu proper renders at this share of the viewport scale; the legend keeps the base scale.
const MENU_SCALE: f32 = 0.92;
/// The row column never grows past 12 row slots.
const MAX_VIEW_ROWS: i32 = 12;
/// Held-direction repeat (seconds).
const REPEAT_DELAY: f32 = 0.42;
const REPEAT_RATE: f32 = 0.11;
/// The recomp's sizes are ImGui line heights; Bevy's `font_size` is the em (Inter: upm / (ascent - descent)).
const EM_PER_SIZE: f32 = 2048. / 2478.;
/// Pooled entities: rail items and visible row slots.
const RAIL_POOL: usize = 16;
const SLOT_POOL: usize = MAX_VIEW_ROWS as usize + 4;

const fn rgba(r: u8, g: u8, b: u8, a: u8) -> Color {
    Color::Srgba(Srgba { red: r as f32 / 255., green: g as f32 / 255., blue: b as f32 / 255., alpha: a as f32 / 255. })
}
// The recomp's palette (kCol*).
const SEL_FILL: Color = rgba(10, 12, 12, 255);
const SEL_TEXT: Color = rgba(250, 252, 252, 255);
const PANEL: Color = rgba(235, 236, 236, 255);
const PANEL_HOVER: Color = rgba(248, 250, 250, 255);
const PANEL_BORDER: Color = rgba(0, 0, 0, 26);
const ROW_TEXT: Color = rgba(15, 17, 18, 255);
const RAIL_PANEL: Color = rgba(14, 56, 53, 140);
const RAIL_PANEL_HOVER: Color = rgba(24, 90, 86, 166);
const RAIL_BORDER: Color = rgba(255, 255, 255, 26);
const TEXT: Color = rgba(235, 242, 241, 255);
const TEXT_DIM: Color = rgba(224, 235, 234, 191);
const TEXT_FAINT: Color = rgba(150, 158, 158, 255);
const ACCENT: Color = rgba(213, 235, 10, 255);
const ACCENT_DARK: Color = rgba(13, 15, 5, 255);
const INTERACT: Color = rgba(230, 0, 120, 255);
const INTERACT_HOVER: Color = rgba(255, 25, 145, 255);
const DANGER: Color = rgba(233, 88, 76, 255);
const DANGER_TEXT: Color = rgba(255, 250, 249, 255);
const WARN: Color = rgba(245, 197, 87, 255);
const DESC_PANEL: Color = rgba(11, 46, 43, 140);
const CHEV_DISABLED: Color = rgba(0, 0, 0, 56);
const CHEV_DISABLED_ON_DARK: Color = rgba(255, 255, 255, 64);
const LEGEND_CHIP: Color = rgba(238, 240, 240, 255);
const LEGEND_TEXT: Color = rgba(15, 18, 20, 255);
const LEGEND_LABEL: Color = rgba(228, 236, 235, 255);
const TRACK: Color = rgba(0, 0, 0, 56);
const TRACK_ON_DARK: Color = rgba(255, 255, 255, 77);
const BAR_TRACK: Color = rgba(255, 255, 255, 18);
const BAR_THUMB: Color = rgba(255, 255, 255, 90);
/// The uniform dark wash over the (blurred) world.
const BACKDROP: Color = rgba(6, 9, 11, 133);

/// Whole pixels keep 1 px rules and text edges crisp.
fn snap(value: f32) -> f32 {
    (value + 0.5).floor()
}
fn em(size: f32) -> f32 {
    size * EM_PER_SIZE
}

/// What the mouse can hit, from the last drawn frame.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) enum Hit {
    Rail(usize),
    Quit,
    Row(usize),
    Left(usize),
    Right(usize),
    /// A slider row's drag area, with its track's x span.
    Slider(usize, f32, f32),
}

/// Held-direction auto-repeat for one axis (the recomp's `repeat_axis`).
#[derive(Clone, Copy, Debug, Default)]
pub(super) struct Repeat {
    held: i32,
    timer: f32,
}
impl Repeat {
    pub(super) fn step(&mut self, direction: i32, dt: f32) -> i32 {
        if direction == 0 {
            *self = Self::default();
            return 0;
        }
        if direction != self.held {
            *self = Self { held: direction, timer: 0. };
            return direction;
        }
        self.timer += dt;
        if self.timer >= REPEAT_DELAY {
            self.timer -= REPEAT_RATE;
            return direction;
        }
        0
    }
}

/// Focus zones, animation and hit areas.
#[derive(Debug)]
pub(super) struct ViewState {
    /// `Menu::open` as last seen by `interact` (opening resets the view).
    pub(super) shown: bool,
    /// Focus is in the row column (else on the rail).
    pub(super) content: bool,
    /// The rail's focus is on Close Game.
    pub(super) rail_quit: bool,
    /// A text row is taking typing.
    pub(super) editing: bool,
    /// The last navigation came from a pad: the legend shows pad buttons.
    pub(super) pad_active: bool,
    pub(super) prev_pad: u16,
    pub(super) prev_keyboard: u16,
    pub(super) repeat_x: Repeat,
    pub(super) repeat_y: Repeat,
    pub(super) mouse_last: Option<Vec2>,
    pub(super) drag: Option<usize>,
    /// Fractional wheel notches, and whole rows still to scroll (negative = up).
    pub(super) wheel: f32,
    pub(super) scroll_rows: i32,
    /// Bring the focused row into view on the next frame.
    pub(super) follow: bool,
    /// The overlay drew this frame (it blurs the world).
    pub(super) visible: bool,
    hits: Vec<(Hit, Rect)>,
    content_rect: Option<Rect>,
    page_key: Option<(usize, bool, bool, bool, u8, bool)>,
    /// Smoothed positions (-1 = snap on the next frame) and the pitch-locked scroll target.
    highlight_y: f32,
    rail_y: f32,
    scroll: f32,
    scroll_anim: f32,
}
impl Default for ViewState {
    fn default() -> Self {
        Self {
            shown: false, content: false, rail_quit: false, editing: false, pad_active: false,
            prev_pad: 0, prev_keyboard: 0, repeat_x: Repeat::default(), repeat_y: Repeat::default(),
            mouse_last: None, drag: None, wheel: 0., scroll_rows: 0, follow: false, visible: false,
            hits: Vec::new(), content_rect: None, page_key: None,
            highlight_y: -1., rail_y: -1., scroll: 0., scroll_anim: 0.,
        }
    }
}
impl ViewState {
    /// The overlay just opened (the recomp's `Show`): focus on the rail, buttons already held are swallowed.
    pub(super) fn reset(&mut self, held: u16) {
        *self = Self { shown: true, prev_pad: 0xFFFF, prev_keyboard: held & 15, ..default() };
    }
    pub(super) fn hit(&self, point: Vec2) -> Option<Hit> {
        self.hits.iter().find(|(_, r)| r.contains(point)).map(|(h, _)| *h)
    }
    pub(super) fn over_content(&self, point: Vec2) -> bool {
        self.content_rect.is_some_and(|r| r.contains(point))
    }
    pub(super) fn slider_track(&self, id: usize) -> Option<(f32, f32)> {
        self.hits.iter().find_map(|(h, _)| match h { Hit::Slider(i, x0, x1) if *i == id => Some((*x0, *x1)), _ => None })
    }
}

/// The recomp's layout for a window (logical pixels).
#[derive(Clone, Copy, Debug)]
struct Geometry {
    s: f32,
    base: f32,
    rail_x: f32,
    rail_w: f32,
    content_x: f32,
    content_w: f32,
    desc_x: f32,
    desc_w: f32,
    columns_y: f32,
    title_y: f32,
    title_size: f32,
    row_h: f32,
    row_gap: f32,
    pitch: f32,
    view_h: f32,
    rail_item_h: f32,
    desc_panel_h: f32,
    quit_y: f32,
    footer_h: f32,
    label: f32,
    desc: f32,
}
fn geometry(width: f32, height: f32, categories: usize) -> Geometry {
    let base = (height / 1080.).clamp(0.6, 3.);
    let s = base * MENU_SCALE;
    let margin_x = snap((56. * s).max(width * 0.05));
    let rail_w = snap(300. * s);
    let desc_w = snap(300. * s);
    let col_gap = snap(14. * s);
    let footer_h = snap(96. * base);
    let content_bottom = snap(height - footer_h - 18. * s);
    let content_w = snap((width - 2. * margin_x - rail_w - desc_w - 2. * col_gap).max(300. * s).min(800. * s));
    let total_w = rail_w + content_w + desc_w + 2. * col_gap;
    let rail_x = snap(margin_x.max((width - total_w) * 0.5));
    let content_x = rail_x + rail_w + col_gap;
    let desc_x = content_x + content_w + col_gap;
    let row_h = snap(52. * s);
    let row_gap = snap(5. * s);
    let rail_item_h = snap(52. * s);
    let pitch = row_h + row_gap;
    // An exact number of row slots, one fewer than fits, counted from the classic anchor (title at 13%)...
    let columns_y_flow = snap(snap(height * 0.13) + 74. * s);
    let view_rows = (((content_bottom - columns_y_flow + row_gap) / pitch) as i32 - 1).clamp(1, MAX_VIEW_ROWS);
    let view_h = view_rows as f32 * pitch - row_gap;
    // ...then the block is centred vertically, the title 74 px above it.
    let low = (98. * s).min(columns_y_flow);
    let columns_y = snap(((height - view_h) * 0.5).max(low).min(content_bottom - view_h));
    let title_y = snap(columns_y - 74. * s);
    // The description panel's bottom lines up with Close Game's.
    let rail_bottom = columns_y + categories as f32 * (rail_item_h + row_gap) - row_gap;
    let rail_close_bottom = snap(rail_bottom + 45. * s) + rail_item_h;
    let desc_panel_h = snap((content_bottom - columns_y).min((320. * s).max(rail_close_bottom - columns_y)));
    let quit_y = snap((columns_y + desc_panel_h - rail_item_h).max(rail_bottom + 45. * s));
    Geometry {
        s, base, rail_x, rail_w, content_x, content_w, desc_x, desc_w, columns_y, title_y, title_size: 42. * s,
        row_h, row_gap, pitch, view_h, rail_item_h, desc_panel_h, quit_y, footer_h, label: 22. * s, desc: 20. * s,
    }
}

/// The black fill with a lime ring and a black outer edge, drawn outside the box (the recomp's stacked shadows).
#[derive(Clone, Copy)]
struct Highlight {
    outer: Entity,
    ring: Entity,
    fill: Entity,
}
/// A text in a box that centres it vertically.
#[derive(Clone, Copy)]
struct Label {
    frame: Entity,
    text: Entity,
}
/// One row slot of the content column.
#[derive(Clone, Copy)]
struct Slot {
    panel: Entity,
    content: Entity,
    label: Label,
    value_area: Entity,
    chevrons: [(Entity, Entity); 2],
    center: Label,
    track: Entity,
    fill: Entity,
    knob: Entity,
    number: Label,
    right: Label,
}
#[derive(Clone, Copy)]
struct LegendItem {
    item: Entity,
    chip: Entity,
    glyph: Entity,
    /// Pad mode: up to two button glyphs (SDL labels for the connected pad) in place of the text chip.
    pads: [Entity; 2],
    label: Entity,
}
#[derive(Resource)]
pub(super) struct OverlayUi {
    root: Entity,
    title: Label,
    chip: Entity,
    chip_text: Entity,
    rail_panels: Vec<Entity>,
    rail_labels: Vec<Label>,
    rail_arrows: Vec<Entity>,
    quit_panel: Entity,
    quit_label: Label,
    rail_highlight: Highlight,
    clip: Entity,
    slots: Vec<Slot>,
    row_highlight: Highlight,
    bar_track: Entity,
    bar_thumb: Entity,
    desc_header: Entity,
    desc_header_label: Label,
    desc_body: Entity,
    desc_texts: [Entity; 4],
    desc_frame: Entity,
    legend: Entity,
    legend_items: Vec<LegendItem>,
}

/// A white right-pointing triangle filling its texture (tinted per use, flipped for left).
fn triangle() -> Image {
    const N: u32 = 64;
    let mut data = Vec::with_capacity((N * N * 4) as usize);
    for y in 0..N {
        for x in 0..N {
            let mut covered = 0u32;
            for sy in 0..4 {
                for sx in 0..4 {
                    let px = (x as f32 + (sx as f32 + 0.5) / 4.) / N as f32;
                    let py = (y as f32 + (sy as f32 + 0.5) / 4.) / N as f32;
                    covered += ((py - 0.5).abs() <= 0.5 * (1. - px)) as u32;
                }
            }
            data.extend_from_slice(&[255, 255, 255, (covered * 255 / 16) as u8]);
        }
    }
    Image::new(
        Extent3d { width: N, height: N, depth_or_array_layers: 1 },
        TextureDimension::D2,
        data,
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::RENDER_WORLD,
    )
}

fn absolute() -> Node {
    Node { position_type: PositionType::Absolute, ..default() }
}

pub(super) fn spawn(commands: &mut Commands, output: Entity, fonts: &mut Assets<Font>, images: &mut Assets<Image>) {
    // Inter (SIL Open Font License, `fonts/OFL.txt`): the recomp overlay's typeface.
    let regular = fonts.add(Font::try_from_bytes(include_bytes!("../fonts/Inter-Regular.ttf").to_vec()).expect("embedded Inter Regular"));
    let bold = fonts.add(Font::try_from_bytes(include_bytes!("../fonts/Inter-SemiBold.ttf").to_vec()).expect("embedded Inter SemiBold"));
    let triangle = images.add(triangle());
    let root = commands
        .spawn((
            Name::new("PC overlay"),
            UiTargetCamera(output),
            GlobalZIndex(10),
            Node { display: Display::None, width: percent(100), height: percent(100), ..absolute() },
            BackgroundColor(BACKDROP),
        ))
        .id();
    let spawn_node = |commands: &mut Commands, parent: Entity, color: Color, z: i32| {
        commands.spawn((absolute(), BackgroundColor(color), ZIndex(z), ChildOf(parent))).id()
    };
    let text = |commands: &mut Commands, parent: Entity, font: &Handle<Font>, layout: TextLayout| {
        commands
            .spawn((Text::new(""), TextFont { font: font.clone(), font_size: 12., ..default() }, TextColor(TEXT), layout, ChildOf(parent)))
            .id()
    };
    let nowrap = TextLayout::new(Justify::Left, LineBreak::NoWrap);
    let label = |commands: &mut Commands, parent: Entity, font: &Handle<Font>, justify: JustifyContent| {
        let frame = commands
            .spawn((Node { align_items: AlignItems::Center, justify_content: justify, overflow: Overflow::clip(), ..absolute() }, ChildOf(parent)))
            .id();
        let text = text(commands, frame, font, nowrap);
        Label { frame, text }
    };
    let highlight = |commands: &mut Commands, parent: Entity, z: i32| {
        let outer = commands.spawn((absolute(), BackgroundColor(SEL_FILL), ZIndex(z), ChildOf(parent))).id();
        let ring = commands.spawn((absolute(), BackgroundColor(ACCENT), ChildOf(outer))).id();
        let fill = commands.spawn((absolute(), BackgroundColor(SEL_FILL), ChildOf(ring))).id();
        Highlight { outer, ring, fill }
    };
    let title = label(commands, root, &bold, JustifyContent::FlexStart);
    let chip = commands.spawn((absolute(), BorderColor::all(WARN), ChildOf(root))).id();
    let chip_text = text(commands, chip, &bold, nowrap);
    // Rail: panels, then the sliding highlight, then labels and arrows on top.
    let rail_panels: Vec<Entity> =
        (0..RAIL_POOL).map(|_| commands.spawn((absolute(), BackgroundColor(RAIL_PANEL), BorderColor::all(RAIL_BORDER), ChildOf(root))).id()).collect();
    let quit_panel = commands.spawn((absolute(), BackgroundColor(RAIL_PANEL), BorderColor::all(RAIL_BORDER), ChildOf(root))).id();
    let rail_highlight = highlight(commands, root, 0);
    let rail_labels: Vec<Label> = (0..RAIL_POOL).map(|_| label(commands, root, &bold, JustifyContent::FlexStart)).collect();
    let rail_arrows: Vec<Entity> = (0..RAIL_POOL)
        .map(|_| commands.spawn((absolute(), ImageNode::new(triangle.clone()).with_color(ACCENT), ChildOf(root))).id())
        .collect();
    let quit_label = label(commands, root, &bold, JustifyContent::FlexStart);
    // Content column, clipped a ring's width outside the rows.
    let clip = commands.spawn((Node { overflow: Overflow::clip(), ..absolute() }, ChildOf(root))).id();
    let slots: Vec<Slot> = (0..SLOT_POOL)
        .map(|_| {
            let panel = commands.spawn((absolute(), BackgroundColor(PANEL), BorderColor::all(PANEL_BORDER), ZIndex(0), ChildOf(clip))).id();
            let content = commands.spawn((absolute(), ZIndex(1), ChildOf(clip))).id();
            let row_label = label(commands, content, &bold, JustifyContent::FlexStart);
            let value_area = commands.spawn((absolute(), ChildOf(content))).id();
            let chevron = |commands: &mut Commands, flip: bool| {
                let circle = commands.spawn((absolute(), BackgroundColor(INTERACT), ChildOf(value_area))).id();
                let mut image = ImageNode::new(triangle.clone());
                image.flip_x = flip;
                let tri = commands.spawn((absolute(), image, ChildOf(circle))).id();
                (circle, tri)
            };
            let chevrons = [chevron(commands, true), chevron(commands, false)];
            let center = label(commands, value_area, &bold, JustifyContent::Center);
            let track = spawn_node(commands, value_area, TRACK, 0);
            let fill = spawn_node(commands, value_area, INTERACT, 0);
            let knob = spawn_node(commands, value_area, INTERACT, 0);
            let number = label(commands, value_area, &bold, JustifyContent::FlexEnd);
            let right = label(commands, value_area, &bold, JustifyContent::FlexEnd);
            Slot { panel, content, label: row_label, value_area, chevrons, center, track, fill, knob, number, right }
        })
        .collect();
    let row_highlight = highlight(commands, clip, 2);
    let bar_track = spawn_node(commands, root, BAR_TRACK, 0);
    let bar_thumb = spawn_node(commands, root, BAR_THUMB, 0);
    // Description panel.
    let desc_header = spawn_node(commands, root, ACCENT, 0);
    let desc_header_label = label(commands, desc_header, &bold, JustifyContent::FlexStart);
    let desc_body = commands
        .spawn((Node { flex_direction: FlexDirection::Column, ..absolute() }, BackgroundColor(DESC_PANEL), ChildOf(root)))
        .id();
    let paragraph = TextLayout::new(Justify::Left, LineBreak::WordBoundary);
    let desc_texts = [
        text(commands, desc_body, &regular, paragraph),
        text(commands, desc_body, &regular, paragraph),
        text(commands, desc_body, &bold, paragraph),
        text(commands, desc_body, &regular, paragraph),
    ];
    let desc_frame = commands.spawn((absolute(), BorderColor::all(RAIL_BORDER), ChildOf(root))).id();
    // Footer legend.
    let legend = commands.spawn((Node { align_items: AlignItems::Center, ..absolute() }, ChildOf(root))).id();
    let legend_items = (0..5)
        .map(|_| {
            let item = commands.spawn((Node { align_items: AlignItems::Center, ..default() }, ChildOf(legend))).id();
            let chip = commands
                .spawn((Node { align_items: AlignItems::Center, justify_content: JustifyContent::Center, ..default() }, BackgroundColor(LEGEND_CHIP), ChildOf(item)))
                .id();
            let glyph = text(commands, chip, &bold, nowrap);
            let pads = [0, 1].map(|_| {
                let node = Node { display: Display::None, ..default() };
                commands.spawn((node, ImageNode::default(), crate::prompt_glyphs::PromptGlyph("A"), ChildOf(item))).id()
            });
            let label = text(commands, item, &bold, nowrap);
            LegendItem { item, chip, glyph, pads, label }
        })
        .collect();
    commands.insert_resource(OverlayUi {
        root, title, chip, chip_text, rail_panels, rail_labels, rail_arrows, quit_panel, quit_label, rail_highlight, clip, slots,
        row_highlight, bar_track, bar_thumb, desc_header, desc_header_label, desc_body, desc_texts, desc_frame, legend,
        legend_items,
    });
}

#[derive(QueryData)]
#[query_data(mutable)]
pub(super) struct Part {
    node: &'static mut Node,
    background: Option<&'static mut BackgroundColor>,
    border: Option<&'static mut BorderColor>,
    text: Option<&'static mut Text>,
    color: Option<&'static mut TextColor>,
    font: Option<&'static mut TextFont>,
    z: Option<&'static mut ZIndex>,
    image: Option<&'static mut ImageNode>,
}

/// Writes that only touch a component when the value changes (no relayout for an unchanged frame).
struct Paint<'a, 'w, 's> {
    q: &'a mut Query<'w, 's, Part>,
}
impl Paint<'_, '_, '_> {
    fn node(&mut self, e: Entity, f: impl FnOnce(&mut Node)) {
        if let Ok(mut p) = self.q.get_mut(e) {
            let mut node = p.node.clone();
            f(&mut node);
            if *p.node != node {
                *p.node = node;
            }
        }
    }
    fn hide(&mut self, e: Entity) {
        self.node(e, |n| n.display = Display::None);
    }
    /// Show at a rectangle (relative to the parent).
    fn rect(&mut self, e: Entity, x: f32, y: f32, w: f32, h: f32) {
        self.node(e, |n| {
            n.display = Display::Flex;
            n.left = Val::Px(x);
            n.top = Val::Px(y);
            n.width = Val::Px(w.max(0.));
            n.height = Val::Px(h.max(0.));
        });
    }
    fn background(&mut self, e: Entity, color: Color) {
        if let Ok(mut p) = self.q.get_mut(e) {
            if let Some(b) = p.background.as_mut() {
                if b.0 != color {
                    b.0 = color;
                }
            }
        }
    }
    fn border(&mut self, e: Entity, color: Color, width: f32) {
        self.node(e, |n| n.border = UiRect::all(Val::Px(width)));
        if let Ok(mut p) = self.q.get_mut(e) {
            if let Some(b) = p.border.as_mut() {
                if **b != BorderColor::all(color) {
                    **b = BorderColor::all(color);
                }
            }
        }
    }
    fn radius(&mut self, e: Entity, radius: f32) {
        self.node(e, |n| n.border_radius = BorderRadius::all(Val::Px(radius)));
    }
    fn text(&mut self, e: Entity, value: &str, color: Color, size: f32) {
        if let Ok(mut p) = self.q.get_mut(e) {
            if let Some(t) = p.text.as_mut() {
                if t.0 != value {
                    t.0 = value.to_owned();
                }
            }
            if let Some(c) = p.color.as_mut() {
                if c.0 != color {
                    c.0 = color;
                }
            }
            if let Some(f) = p.font.as_mut() {
                if f.font_size != size {
                    f.font_size = size;
                }
            }
        }
    }
    fn z(&mut self, e: Entity, z: i32) {
        if let Ok(mut p) = self.q.get_mut(e) {
            if let Some(i) = p.z.as_mut() {
                if i.0 != z {
                    i.0 = z;
                }
            }
        }
    }
    /// A vertically centred label filling a rectangle.
    fn label(&mut self, l: Label, x: f32, y: f32, w: f32, h: f32, value: &str, color: Color, size: f32) {
        self.rect(l.frame, x, y, w, h);
        self.text(l.text, value, color, em(size));
    }
    fn highlight(&mut self, h: Highlight, x: f32, y: f32, w: f32, height: f32, s: f32) {
        // Fill radius 6, lime band to +2 (radius 8), black edge to +6 (radius 12).
        self.rect(h.outer, x - 6. * s, y - 6. * s, w + 12. * s, height + 12. * s);
        self.radius(h.outer, 12. * s);
        self.rect(h.ring, 4. * s, 4. * s, w + 4. * s, height + 4. * s);
        self.radius(h.ring, 8. * s);
        self.rect(h.fill, 2. * s, 2. * s, w, height);
        self.radius(h.fill, 6. * s);
    }
}

/// Moves `value` toward `target` at the recomp's rate, settling exactly when close.
fn chase(value: &mut f32, target: f32, rate: f32, dt: f32, snap_distance: f32) {
    if *value < 0. || (*value - target).abs() > snap_distance {
        *value = target;
    }
    *value += (target - *value) * (dt * rate).min(1.);
    if (*value - target).abs() < 0.5 {
        *value = target;
    }
}

#[allow(clippy::too_many_arguments)]
pub(super) fn view(
    mut menu: ResMut<Menu>,
    ui: Res<OverlayUi>,
    real: Res<Time<Real>>,
    window: Single<&Window, With<PrimaryWindow>>,
    (travel, customiser, custom_models, mods_menu): (
        Res<crate::teleport_menu::Travel>,
        Res<crate::customiser::Customiser>,
        Res<crate::custom_models::CustomModels>,
        Res<crate::modding::ModMenu>,
    ),
    (audio, camera, net, controllers, transition): (
        Option<Res<crate::game_audio::AudioSettings>>,
        Res<crate::camera::CameraAngleSettings>,
        Res<crate::multiplayer::Multiplayer>,
        Res<crate::input::ControllerInput>,
        Res<crate::map_transition::MapTransition>,
    ),
    (mods, physics, appearances, living, water_look): (Res<crate::modding::Mods>, Res<crate::physics::GamePhysics>, Res<crate::multiplayer::appearance::Appearances>, Option<Res<crate::living_world::LivingWorldSettings>>, Option<Res<crate::water_look::WaterLookSettings>>),
    mut parts: Query<Part>,
    mut glyphs: Query<&mut crate::prompt_glyphs::PromptGlyph>,
) {
    let mut paint = Paint { q: &mut parts };
    let showing = menu.open && !travel.open && !customiser.open && !custom_models.open && !mods_menu.open;
    menu.view.visible = showing;
    if !showing {
        paint.hide(ui.root);
        menu.view.hits.clear();
        menu.view.content_rect = None;
        return;
    }
    paint.node(ui.root, |n| n.display = Display::Flex);
    let (width, height) = (window.width(), window.height());
    let categories = menu.category_count();
    let g = geometry(width, height, categories);
    let s = g.s;
    let dt = real.delta_secs().min(0.25);
    let cursor = window.cursor_position();
    let inside = |x0: f32, y0: f32, x1: f32, y1: f32| cursor.is_some_and(|c| c.x >= x0 && c.x < x1 && c.y >= y0 && c.y < y1);

    let mut debug_rows = Vec::new();
    if menu.multiplayer && menu.network_page == 4 {
        debug_rows.extend(net.debug_sections());
        debug_rows.push(format!("CHARACTERS & COLLISIONS\n{} {}\nPlayer contacts: {} | Network active: {}",
            appearances.progress, appearances.status, physics.network_contacts, physics.network_active));
        debug_rows.extend(mods.multiplayer_debug_sections());
    }
    let page = menu.page();
    let views: Vec<RowView> = {
        let cx = RowCtx {
            audio: audio.as_deref(),
            camera: &camera,
            net: &net,
            controllers: Some(&controllers),
            debug: &debug_rows,
            window: window.physical_size(),
            opt_ins: living.as_deref().map(crate::living_world::opt_ins::in_effect).unwrap_or_default(),
            better_water: water_look.as_deref().map_or((false, None), |w| (w.selected.enabled, w.patched_by().map(str::to_owned))),
        };
        page.iter().map(|id| menu.describe(*id, &cx)).collect()
    };
    let menu: &mut Menu = &mut menu;
    let focus = if menu.view.content { page.iter().position(|id| *id == menu.selected) } else { None };
    let key = (menu.section, menu.map_detail, menu.daylight, menu.browser, menu.network_page, menu.multiplayer);
    let title = menu.title();
    let v = &mut menu.view;
    v.hits.clear();
    if v.page_key != Some(key) {
        v.page_key = Some(key);
        v.scroll = 0.;
        v.scroll_anim = 0.;
        v.highlight_y = -1.;
    }

    // ---- Title and the pending-changes chip ----
    paint.label(ui.title, g.rail_x, g.title_y, g.desc_x + g.desc_w - g.rail_x, snap(g.title_size), &title, TEXT, g.title_size);
    let chip_size = 14. * s;
    let menu_right = g.desc_x + g.desc_w;
    let stale = !crate::setup::stale_groups().is_empty();
    if stale || menu.custom_dirty {
        let chip_h = chip_size + 12. * s;
        paint.node(ui.chip, |n| {
            n.display = Display::Flex;
            n.left = Val::Auto;
            n.right = Val::Px(width - menu_right);
            n.top = Val::Px(snap(g.title_y + g.title_size * 0.5 - chip_h * 0.5));
            n.padding = UiRect::axes(Val::Px(10. * s), Val::Px(6. * s));
        });
        paint.border(ui.chip, WARN, 1.5);
        paint.text(ui.chip_text, if stale { "GAME FILES OUT OF DATE" } else { "TUNING NOT APPLIED" }, WARN, em(chip_size));
    } else {
        paint.hide(ui.chip);
    }

    // ---- Left rail ----
    let section = menu.section;
    let v = &mut menu.view;
    let rail_target = (!v.content && !v.rail_quit).then(|| g.columns_y + section as f32 * (g.rail_item_h + g.row_gap));
    let names: Vec<String> = (0..categories.min(RAIL_POOL)).map(|i| menu.category_name(i).to_owned()).collect();
    let v = &mut menu.view;
    for i in 0..RAIL_POOL {
        let (panel, label, arrow) = (ui.rail_panels[i], ui.rail_labels[i], ui.rail_arrows[i]);
        if i >= names.len() {
            paint.hide(panel);
            paint.hide(label.frame);
            paint.hide(arrow);
            continue;
        }
        let y0 = g.columns_y + i as f32 * (g.rail_item_h + g.row_gap);
        let y1 = y0 + g.rail_item_h;
        let hovered = inside(g.rail_x, y0, g.rail_x + g.rail_w, y1);
        let current = section == i;
        let rail_focused = current && rail_target.is_some();
        let arrived = v.rail_y >= 0. && (v.rail_y - y0).abs() < 0.5;
        if rail_focused && arrived {
            // The rounded fill covers it; its square corners would poke out.
            paint.hide(panel);
        } else {
            paint.rect(panel, g.rail_x, y0, g.rail_w, g.rail_item_h);
            paint.background(panel, if current && !rail_focused { SEL_FILL } else if hovered { RAIL_PANEL_HOVER } else { RAIL_PANEL });
            paint.border(panel, RAIL_BORDER, 1.);
        }
        v.hits.push((Hit::Rail(i), Rect::new(g.rail_x, y0, g.rail_x + g.rail_w, y1)));
        // Focused text waits until the sliding fill mostly covers the item.
        let focused = rail_focused && v.rail_y >= 0. && (v.rail_y - y0).abs() < g.rail_item_h * 0.5;
        let color = if focused { SEL_TEXT } else if current { TEXT } else { TEXT_DIM };
        paint.label(label, g.rail_x + 20. * s, y0, g.rail_w - 40. * s, g.rail_item_h, &names[i], color, g.label);
        if current && v.content {
            // Focus lives in the row column.
            let (ax, ay) = (g.rail_x + g.rail_w - 18. * s, y0 + g.rail_item_h * 0.5);
            paint.rect(arrow, ax, ay - 6. * s, 8. * s, 12. * s);
        } else {
            paint.hide(arrow);
        }
    }
    match rail_target {
        Some(target) => {
            chase(&mut v.rail_y, target, 22., dt, 160. * s);
            paint.highlight(ui.rail_highlight, g.rail_x, snap(v.rail_y), g.rail_w, g.rail_item_h, s);
        }
        None => {
            v.rail_y = -1.;
            paint.hide(ui.rail_highlight.outer);
        }
    }
    {
        let y0 = g.quit_y;
        let y1 = y0 + g.rail_item_h;
        let focused = !v.content && v.rail_quit;
        let hovered = inside(g.rail_x, y0, g.rail_x + g.rail_w, y1);
        paint.rect(ui.quit_panel, g.rail_x, y0, g.rail_w, g.rail_item_h);
        paint.background(ui.quit_panel, if focused { DANGER } else if hovered { RAIL_PANEL_HOVER } else { RAIL_PANEL });
        paint.border(ui.quit_panel, if focused { DANGER } else { RAIL_BORDER }, 1.);
        paint.label(ui.quit_label, g.rail_x + 20. * s, y0, g.rail_w - 40. * s, g.rail_item_h, "Close Game", if focused { DANGER_TEXT } else { DANGER }, g.label);
        v.hits.push((Hit::Quit, Rect::new(g.rail_x, y0, g.rail_x + g.rail_w, y1)));
    }

    // ---- Content column ----
    let row_y: Vec<f32> = (0..page.len()).map(|i| i as f32 * g.pitch).collect();
    let total_h = if page.is_empty() { 0. } else { page.len() as f32 * g.pitch - g.row_gap };
    let max_scroll = (total_h - g.view_h).max(0.);
    v.scroll += v.scroll_rows as f32 * g.pitch;
    v.scroll_rows = 0;
    if std::mem::take(&mut v.follow) {
        if let Some(i) = focus {
            let (y0, y1) = (row_y[i], row_y[i] + g.row_h);
            if y0 < v.scroll {
                v.scroll = y0;
            } else if y1 - v.scroll > g.view_h {
                v.scroll = y1 - g.view_h;
            }
        }
    }
    // The resting position locks to whole rows; the drawn offset slides after it.
    v.scroll = ((v.scroll.clamp(0., max_scroll) / g.pitch).round() * g.pitch).clamp(0., max_scroll);
    if v.scroll_anim < 0. {
        v.scroll_anim = v.scroll;
    }
    v.scroll_anim += (v.scroll - v.scroll_anim) * (dt * 16.).min(1.);
    if (v.scroll_anim - v.scroll).abs() < 0.5 {
        v.scroll_anim = v.scroll;
    }
    let scroll = snap(v.scroll_anim);
    let ring = 6. * s;
    let view_bottom = g.columns_y + g.view_h;
    paint.rect(ui.clip, g.content_x - ring, g.columns_y - ring, g.content_w + 2. * ring + 1., g.view_h + 2. * ring);
    v.content_rect = Some(Rect::new(g.content_x, g.columns_y, g.content_x + g.content_w, view_bottom));
    // Children of the clip box sit `ring` in from its corner.
    let local = |x: f32, y: f32| (x - g.content_x + ring, y - g.columns_y + ring);
    match focus {
        Some(i) => chase(&mut v.highlight_y, row_y[i], 22., dt, 160. * s),
        None => v.highlight_y = -1.,
    }
    let value_w = (390. * s).min(g.content_w * 0.5);
    let chevron_r = 13. * s;
    let mut slot = 0;
    for (i, row) in views.iter().enumerate() {
        let y0 = g.columns_y + snap(row_y[i]) - scroll;
        let y1 = y0 + g.row_h;
        if y1 < g.columns_y || y0 > view_bottom || slot >= ui.slots.len() {
            continue;
        }
        let ui_slot = ui.slots[slot];
        slot += 1;
        let id = page[i];
        let (lx, ly) = local(g.content_x, y0);
        let in_view = |y: f32| y >= g.columns_y && y <= view_bottom;
        let hovered = inside(g.content_x, y0.max(g.columns_y), g.content_x + g.content_w, y1.min(view_bottom));
        let focus_row = focus == Some(i);
        let focused = focus_row && (v.highlight_y - row_y[i]).abs() < g.row_h * 0.5;
        paint.rect(ui_slot.content, lx, ly, g.content_w, g.row_h);
        paint.z(ui_slot.content, if focus_row { 3 } else { 1 });
        if row.kind == Kind::Header {
            paint.rect(ui_slot.panel, lx, ly, g.content_w, g.row_h);
            paint.background(ui_slot.panel, ACCENT);
            paint.border(ui_slot.panel, ACCENT, 0.);
            paint.label(ui_slot.label, 18. * s, 0., g.content_w - 36. * s, g.row_h, &row.label, ACCENT_DARK, g.label);
            paint.hide(ui_slot.value_area);
            continue;
        }
        if focus_row && (v.highlight_y - row_y[i]).abs() < 0.5 {
            paint.hide(ui_slot.panel);
        } else {
            paint.rect(ui_slot.panel, lx, ly, g.content_w, g.row_h);
            paint.background(ui_slot.panel, if hovered && row.enabled { PANEL_HOVER } else { PANEL });
            paint.border(ui_slot.panel, PANEL_BORDER, 1.);
        }
        let label_color = if focused { SEL_TEXT } else if !row.enabled { TEXT_FAINT } else if row.danger { DANGER } else { ROW_TEXT };
        let value_color = if focused { SEL_TEXT } else if row.enabled { ROW_TEXT } else { TEXT_FAINT };
        let has_value = !matches!(row.kind, Kind::Action | Kind::Text | Kind::Info) || !row.value.is_empty();
        let label_w = if has_value { g.content_w - value_w - 26. * s } else { g.content_w - 36. * s };
        paint.label(ui_slot.label, 18. * s, 0., label_w, g.row_h, &row.label, label_color, g.label);
        // Value area: the right side of the row, `value_w` wide, 12 px in from the edge.
        let area_w = value_w - 12. * s;
        let area_x = g.content_w - value_w;
        paint.rect(ui_slot.value_area, area_x, 0., area_w, g.row_h);
        let cy = g.row_h * 0.5;
        let abs_x = g.content_x + area_x;
        let hide_all = |paint: &mut Paint, keep: &[Entity]| {
            for e in [ui_slot.chevrons[0].0, ui_slot.chevrons[1].0, ui_slot.center.frame, ui_slot.track, ui_slot.fill, ui_slot.knob, ui_slot.number.frame, ui_slot.right.frame] {
                if !keep.contains(&e) {
                    paint.hide(e);
                }
            }
        };
        match row.kind {
            Kind::Enum { .. } => {
                hide_all(&mut paint, &[ui_slot.chevrons[0].0, ui_slot.chevrons[1].0, ui_slot.center.frame]);
                for (side, (circle, tri)) in ui_slot.chevrons.into_iter().enumerate() {
                    let direction = if side == 0 { -1 } else { 1 };
                    let cx = if side == 0 { chevron_r } else { area_w - chevron_r };
                    let (hx, hy) = (abs_x + cx, y0 + cy);
                    let r = chevron_r + 4.;
                    let hit = inside(hx - r, hy - r, hx + r, hy + r) && in_view(hy);
                    let can = row.can_step(direction);
                    paint.rect(circle, snap(cx - chevron_r), snap(cy - chevron_r), 2. * chevron_r, 2. * chevron_r);
                    paint.radius(circle, chevron_r);
                    paint.background(circle, if !can { if focused { CHEV_DISABLED_ON_DARK } else { CHEV_DISABLED } } else if hit { INTERACT_HOVER } else { INTERACT });
                    // Tip-anchored triangle: the tip reaches 0.42 r from the centre, the back 0.6 of that behind it.
                    let t = chevron_r * 0.42;
                    let back = if side == 0 { chevron_r - t } else { chevron_r - t * 0.6 };
                    paint.rect(tri, back, chevron_r - t, t * 1.6, 2. * t);
                    if in_view(hy) {
                        v.hits.push((if side == 0 { Hit::Left(id) } else { Hit::Right(id) }, Rect::new(hx - r, hy - r, hx + r, hy + r)));
                    }
                }
                let text_w = (area_w - 2. * chevron_r) - 2. * chevron_r - 16. * s;
                paint.label(ui_slot.center, 2. * chevron_r + 8. * s, 0., text_w, g.row_h, &row.value, value_color, g.label);
            }
            Kind::Slider { t } => {
                hide_all(&mut paint, &[ui_slot.track, ui_slot.fill, ui_slot.knob, ui_slot.number.frame]);
                let number_w = 52. * s;
                let (x0, x1) = (6. * s, area_w - number_w);
                let knob_x = x0 + t.clamp(0., 1.) * (x1 - x0);
                paint.rect(ui_slot.track, x0, cy - 2. * s, x1 - x0, 4. * s);
                paint.background(ui_slot.track, if focused { TRACK_ON_DARK } else { TRACK });
                paint.rect(ui_slot.fill, x0, cy - 2. * s, knob_x - x0, 4. * s);
                let k = 8. * s;
                paint.rect(ui_slot.knob, snap(knob_x) - k, snap(cy) - k, 2. * k, 2. * k);
                paint.radius(ui_slot.knob, k);
                paint.label(ui_slot.number, x1, 0., number_w, g.row_h, &row.value, value_color, g.label);
                if in_view(y0 + cy) {
                    let (ax0, ax1) = (abs_x + x0, abs_x + x1);
                    v.hits.push((Hit::Slider(id, ax0, ax1), Rect::new(ax0 - 10. * s, y0 + cy - 14. * s, ax1 + 10. * s, y0 + cy + 14. * s)));
                }
            }
            Kind::Action | Kind::Text | Kind::Info | Kind::Header => {
                hide_all(&mut paint, &[ui_slot.right.frame]);
                let value = if row.kind == Kind::Text && row.value.is_empty() { "-" } else { row.value.as_str() };
                paint.label(ui_slot.right, 0., 0., area_w, g.row_h, value, value_color, g.label);
            }
        }
        if row.enabled {
            v.hits.push((Hit::Row(id), Rect::new(g.content_x, y0.max(g.columns_y), g.content_x + g.content_w, y1.min(view_bottom))));
        }
    }
    for rest in &ui.slots[slot..] {
        paint.hide(rest.panel);
        paint.hide(rest.content);
    }
    // The sliding highlight sits over the other rows and under the focused row's content.
    if let (Some(_), true) = (focus, v.highlight_y >= 0.) {
        let (lx, ly) = local(g.content_x, g.columns_y + snap(v.highlight_y) - scroll);
        paint.highlight(ui.row_highlight, lx, ly, g.content_w, g.row_h, s);
    } else {
        paint.hide(ui.row_highlight.outer);
    }
    // Scrollbar hint when the list overflows.
    if max_scroll > 0. {
        let bar_x = g.content_x + g.content_w + 4. * s;
        let bar_h = (24. * s).max(g.view_h * (g.view_h / total_h));
        let bar_y = g.columns_y + (g.view_h - bar_h) * (v.scroll_anim / max_scroll);
        paint.rect(ui.bar_track, bar_x, g.columns_y, 3. * s, g.view_h);
        paint.rect(ui.bar_thumb, bar_x, bar_y, 3. * s, bar_h);
    } else {
        paint.hide(ui.bar_track);
        paint.hide(ui.bar_thumb);
    }

    // ---- Description panel ----
    let header_h = g.row_h;
    paint.rect(ui.desc_header, g.desc_x, g.columns_y, g.desc_w, header_h);
    paint.label(ui.desc_header_label, 18. * s, 0., g.desc_w - 36. * s, header_h, "Description", ACCENT_DARK, g.label);
    paint.rect(ui.desc_body, g.desc_x, g.columns_y + header_h, g.desc_w, g.desc_panel_h - header_h);
    paint.node(ui.desc_body, |n| {
        n.padding = UiRect::all(Val::Px(14. * s));
        n.row_gap = Val::Px(10. * s);
        n.overflow = Overflow::clip();
    });
    paint.rect(ui.desc_frame, g.desc_x, g.columns_y, g.desc_w, g.desc_panel_h);
    paint.border(ui.desc_frame, RAIL_BORDER, 1.);
    let rail_desc = if v.rail_quit && !v.content { "Quit to the desktop. Unsaved game progress is lost." } else { menu.category_desc(section) };
    let desc = focus.map_or(rail_desc.to_owned(), |i| views[i].desc.clone());
    let status = if transition.busy() {
        format!("{} {}\nGameplay is paused. Please wait.", ["|", "/", "-", "\\"][(real.elapsed_secs() * 4.) as usize % 4], transition.label())
    } else if menu.browser {
        net.browser_status.clone()
    } else if menu.multiplayer {
        format!("{}{}", net.status, if net.host_code.is_empty() { String::new() } else { format!("\nYour connection code: {}", net.host_code) })
    } else {
        menu.status.clone()
    };
    let note = if menu.view.editing { "Typing: Enter or Esc to finish." } else { "" };
    let pending = if menu.custom_dirty {
        "Custom tuning changes are not in use yet. Use Apply Custom Tuning when ready."
    } else if stale {
        "Your game files are out of date: Extras > Refresh Game Files updates them."
    } else {
        ""
    };
    for (e, value, color) in [(ui.desc_texts[0], desc.as_str(), TEXT), (ui.desc_texts[1], status.as_str(), TEXT_DIM), (ui.desc_texts[2], note, WARN), (ui.desc_texts[3], pending, WARN)] {
        if value.is_empty() {
            paint.hide(e);
        } else {
            paint.node(e, |n| { n.display = Display::Flex; n.width = Val::Percent(100.); });
            paint.text(e, value, color, em(g.desc));
        }
    }

    // ---- Footer legend (the base scale, not the menu's) ----
    let fs = g.base;
    let chip_h = snap(26. * fs);
    paint.node(ui.legend, |n| {
        n.display = Display::Flex;
        n.left = Val::Px(g.rail_x);
        n.top = Val::Px(snap(height - g.footer_h + 14. * fs));
        n.height = Val::Px(chip_h);
    });
    let v = &menu.view;
    let reset = focus.is_some_and(|i| views[i].reset);
    // Pad: the connected pad's own glyphs (SDL labels, prompt_glyphs; retail button names). Keyboard: key chips.
    let pad: Vec<(&[&'static str], &str)> = [
        Some((&["A"][..], "Select")),
        Some((&["B"][..], "Back")),
        reset.then_some((&["Y"][..], "Reset to Default")),
        Some((&["L_Bumper", "R_Bumper"][..], "Category")),
        menu.custom_dirty.then_some((&["X"][..], "Apply Tuning")),
    ]
    .into_iter()
    .flatten()
    .collect();
    let keys: Vec<(&str, &str, bool)> = [Some(("Enter", "Select", false)), Some(("Esc", "Back", false)), reset.then_some(("R", "Reset to Default", false)), Some(("Q / E", "Category", false)),
        menu.custom_dirty.then_some(("F", "Apply Tuning", false))].into_iter().flatten().collect();
    for (k, item) in ui.legend_items.iter().enumerate() {
        let count = if v.pad_active { pad.len() } else { keys.len() };
        if k >= count {
            paint.hide(item.item);
            continue;
        }
        paint.node(item.item, |n| {
            n.display = Display::Flex;
            n.column_gap = Val::Px(8. * fs);
            n.margin = UiRect::right(Val::Px(26. * fs));
        });
        if v.pad_active {
            let (buttons, label) = pad[k];
            paint.hide(item.chip);
            for (i, e) in item.pads.iter().enumerate() {
                let Some(button) = buttons.get(i) else {
                    paint.hide(*e);
                    continue;
                };
                paint.node(*e, |n| {
                    n.display = Display::Flex;
                    n.width = Val::Px(chip_h);
                    n.height = Val::Px(chip_h);
                });
                if let Ok(mut g) = glyphs.get_mut(*e) {
                    if g.0 != *button {
                        g.0 = button;
                    }
                }
            }
            paint.text(item.label, label, LEGEND_LABEL, em(16. * fs));
            continue;
        }
        let (glyph, label, circle) = keys[k];
        for e in item.pads {
            paint.hide(e);
        }
        paint.node(item.chip, |n| {
            n.display = Display::Flex;
            n.height = Val::Px(chip_h);
            n.min_width = Val::Px(chip_h);
            n.width = if circle { Val::Px(chip_h) } else { Val::Auto };
            n.padding = if circle { UiRect::ZERO } else { UiRect::axes(Val::Px(9. * fs), Val::ZERO) };
        });
        paint.radius(item.chip, if circle { chip_h * 0.5 } else { 4. * fs });
        paint.text(item.glyph, glyph, LEGEND_TEXT, em(15. * fs));
        paint.text(item.label, label, LEGEND_LABEL, em(16. * fs));
    }
}

/// The world blurs behind the overlay (`fe_backdrop`, at the recomp's strength).
pub(super) fn sync_blur(menu: Res<Menu>, backdrop: Option<ResMut<crate::fe_backdrop::FeBackdrop>>) {
    if let Some(mut backdrop) = backdrop {
        backdrop.set_overlay(menu.view.visible);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn repeat_fires_once_then_after_the_delay_at_the_rate() {
        let mut r = Repeat::default();
        assert_eq!(r.step(1, 0.016), 1, "initial press");
        let mut fired = 0;
        let mut t = 0.;
        while t < 0.42 - 0.02 {
            fired += r.step(1, 0.01).abs();
            t += 0.01;
        }
        assert_eq!(fired, 0, "nothing before the 0.42 s delay");
        let mut fired = 0;
        for _ in 0..(1.0f32 / 0.01) as usize {
            fired += r.step(1, 0.01).abs();
        }
        assert!((8..=10).contains(&fired), "about one per 0.11 s after it, got {fired}");
        assert_eq!(r.step(0, 0.01), 0);
        assert_eq!(r.step(-1, 0.01), -1, "a new direction fires at once");
    }

    #[test]
    fn layout_matches_the_recomp_at_1080p() {
        let g = geometry(1920., 1080., 5);
        // s = 0.92: rail and description 276 wide, rows 48 high with a 5 px gap, 12-slot cap.
        assert_eq!((g.rail_w, g.desc_w, g.row_h, g.row_gap), (276., 276., 48., 5.));
        assert_eq!(g.content_w, 736., "clamped to 800 s");
        assert_eq!(g.rail_x, snap((1920. - (276. + 736. + 276. + 2. * 13.)) * 0.5));
        let rows = ((g.view_h + g.row_gap) / g.pitch).round() as i32;
        assert!((1..=12).contains(&rows));
        assert!(g.columns_y + g.view_h <= 1080. - g.footer_h, "rows clear the legend");
        assert!(g.quit_y >= g.columns_y + 5. * (g.rail_item_h + g.row_gap), "Close Game below the categories");
        // Small windows stop shrinking at 0.6 of the 1080p metrics.
        assert_eq!(geometry(1280., 600., 5).base, 0.6);
    }

    #[test]
    fn chase_settles_exactly_and_snaps_long_jumps() {
        let mut y = -1.;
        chase(&mut y, 100., 22., 0.016, 160.);
        assert_eq!(y, 100., "first frame snaps");
        chase(&mut y, 150., 22., 0.016, 160.);
        assert!(y > 100. && y < 150.);
        for _ in 0..60 {
            chase(&mut y, 150., 22., 0.016, 160.);
        }
        assert_eq!(y, 150.);
        chase(&mut y, 500., 22., 0.016, 160.);
        assert_eq!(y, 500., "beyond 160 px it jumps");
    }
}
