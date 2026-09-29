//! Native-resolution pause UI over a separately scaled 3D render target.
use crate::difficulty::Difficulty;
use bevy::{
    camera::RenderTarget,
    image::ImageSampler,
    prelude::*,
    render::{
        render_resource::{Extent3d, TextureFormat},
    },
    window::{MonitorSelection, PresentMode, PrimaryWindow, VideoModeSelection, WindowMode},
};
use serde::{Deserialize, Serialize};
use std::{
    path::PathBuf,
    time::{Duration, Instant},
};

const RESOLUTIONS: &[(u32, u32)] = &[
    (1280, 720),
    (1280, 800),
    (1600, 900),
    (1920, 1080),
    (1920, 1200),
    (2560, 1440),
    (2560, 1600),
    (3840, 2160),
];
const SCALES: &[u32] = &[25, 50, 67, 75, 85, 100];
const DAY_SPEEDS: &[u32] = &[0, 1, 10, 30, 60, 120, 360, 720];
const LIMITS: &[u32] = &[0, 30, 60, 90, 120, 144, 165, 240];

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
struct GraphicsSettings {
    width: u32,
    height: u32,
    scale: u32,
    fps: u32,
    hour: f32,
    day_speed: u32,
    ambient_level: Option<u32>,
    /// 0 = windowed, 1 = borderless fullscreen, 2 = exclusive fullscreen.
    display_mode: u8,
}
/// Display-mode labels indexed by `GraphicsSettings::display_mode`.
const DISPLAY_MODES: &[&str] = &["Windowed", "Borderless", "Fullscreen"];
impl Default for GraphicsSettings {
    fn default() -> Self {
        Self {
            width: 1280,
            height: 800,
            scale: 100,
            fps: 0,
            hour: 12.,
            day_speed: 60,
            ambient_level: None,
            display_mode: 0,
        }
    }
}
impl GraphicsSettings {
    fn validated(mut self) -> Self {
        self.ambient_level = self.ambient_level.map(|level| level.min(100));
        self.hour = if self.hour.is_finite() { self.hour.rem_euclid(24.) } else { 12. };
        if !DAY_SPEEDS.contains(&self.day_speed) { self.day_speed = 60; }
        if !RESOLUTIONS.contains(&(self.width, self.height)) {
            (self.width, self.height) = (1280, 800);
        }
        if !SCALES.contains(&self.scale) {
            self.scale = 100;
        }
        if !LIMITS.contains(&self.fps) {
            self.fps = 0;
        }
        if self.display_mode > 2 {
            self.display_mode = 0;
        }
        self
    }
    fn internal_size(&self, window: UVec2) -> UVec2 {
        Self::scaled_size(window, self.scale)
    }
    fn scaled_size(window: UVec2, scale: u32) -> UVec2 {
        (window * scale / 100).max(UVec2::ONE)
    }
}

/// Automatic render-scale governor: identical maximum pixels, temporary dips
/// only under sustained GPU pressure. The user's menu setting stays the
/// ceiling and is the only thing that persists; the reduced value never
/// touches the save file and always recovers when headroom returns.
#[derive(Resource)]
pub(crate) struct AutoScale {
    avg_ms: f32,
    over: u32,
    under: u32,
    effective: u32,
    last_user: u32,
    warmup: u32,
}
impl Default for AutoScale {
    fn default() -> Self {
        Self { avg_ms: 16.7, over: 0, under: 0, effective: 100, last_user: 0, warmup: 600 }
    }
}
impl AutoScale {
    const ALPHA: f32 = 0.05;
    const DOWN_MS: f32 = 22.0;
    const DOWN_FRAMES: u32 = 180;
    const UP_MS: f32 = 13.0;
    const UP_FRAMES: u32 = 900;
    const MIN_SCALE: u32 = 50;

    fn index(scale: u32) -> usize {
        SCALES.iter().position(|s| *s == scale).unwrap_or(SCALES.len() - 1)
    }

    pub(crate) fn effective(&self, user_scale: u32) -> u32 {
        self.effective.min(user_scale)
    }

    /// Advance one frame. Returns the new effective scale only when the
    /// governor itself steps (for logging). Adopting a manual setting
    /// change is silent. Map loads, the warmup window and missing/invalid
    /// clocks only adopt and never adapt, so loading hitches can never
    /// trigger a step-down.
    pub(crate) fn update(&mut self, dt_ms: Option<f32>, user_scale: u32, busy: bool) -> Option<u32> {
        if user_scale != self.last_user {
            self.last_user = user_scale;
            self.over = 0;
            self.under = 0;
            self.effective = user_scale;
            return None;
        }
        let Some(dt) = dt_ms.filter(|dt| dt.is_finite() && *dt >= 0.0) else { return None; };
        if busy {
            self.over = 0;
            self.under = 0;
            return None;
        }
        if self.warmup > 0 {
            self.warmup -= 1;
            return None;
        }
        self.avg_ms += (dt - self.avg_ms) * Self::ALPHA;
        if self.avg_ms > Self::DOWN_MS {
            self.under = 0;
            self.over += 1;
            if self.over >= Self::DOWN_FRAMES {
                self.over = 0;
                // Never step below the floor, and never above the setting:
                // a 25% user ceiling stays exactly there under pressure.
                let next = Self::index(self.effective)
                    .saturating_sub(1)
                    .max(Self::index(Self::MIN_SCALE))
                    .min(Self::index(user_scale));
                if SCALES[next] != self.effective {
                    self.effective = SCALES[next];
                    return Some(self.effective);
                }
            }
        } else if self.avg_ms < Self::UP_MS {
            self.over = 0;
            self.under += 1;
            if self.under >= Self::UP_FRAMES {
                self.under = 0;
                let next = (Self::index(self.effective) + 1).min(Self::index(user_scale));
                if SCALES[next] != self.effective {
                    self.effective = SCALES[next];
                    return Some(self.effective);
                }
            }
        } else {
            self.over = 0;
            self.under = 0;
        }
        None
    }
}
#[derive(Resource)]
pub(crate) struct Menu {
    pub(crate) open: bool,
    selected: usize,
    settings: GraphicsSettings,
    path: PathBuf,
    difficulty: Difficulty,
    status: String,
    maps: Vec<crate::map_library::Entry>,
    selected_map: usize,
    multiplayer: bool,
    browser: bool,
    network_page: u8,
    browser_count: usize,
    daylight: bool,
    section: usize,
    custom_sections: Vec<(String, Vec<(String,String,String)>)>,
    map_detail: bool,
    destinations: Vec<crate::teleport_menu::Destination>,
    pending_travel: Option<(Option<PathBuf>, [[f32; 4]; 4])>,
}
impl Menu {
    #[cfg(test)]
    pub(crate) fn advance_day(&mut self, seconds: f32) -> f32 {
        if !self.open && self.settings.day_speed > 0 {
            self.settings.hour = (self.settings.hour + seconds * self.settings.day_speed as f32 / 3600.).rem_euclid(24.);
        }
        self.settings.hour
    }
    pub(crate) fn diagnostic_settings(&self) -> String {
        format!("{:?}", self.settings)
    }
    pub(crate) fn transition_finished(&mut self, status: String, resume: bool) {
        self.status = status;
        self.open = !resume;
    }
}
pub(crate) fn gameplay_active(menu: Option<Res<Menu>>) -> bool {
    menu.is_none_or(|m| !m.open)
}

const SECTIONS: &[(&str, &str)] = &[
    ("MAPS", "Choose a map, then pick your drop-in spot."),
    ("SKATER", "Make it yours."),
    ("GRAPHICS", "Dial in your display and performance."),
    ("MULTIPLAYER", "A session is better with friends."),
    ("EXTRAS", "Mods, updates and more."),
];
#[derive(Component)] struct MenuTitle;
#[derive(Component)] struct MenuSubtitle;
#[derive(Component)] struct MenuScroll;
impl Menu {
    fn rows(&self) -> Vec<usize> {
        if self.daylight { return (0..4).collect(); }
        if self.multiplayer {
            return if self.browser { std::iter::once(0).chain(1..=self.browser_count.min(5)).chain([6,7,8,10]).collect() } else { match self.network_page {
                1 => vec![3,4,10],
                2 => vec![5,7,10],
                3 => vec![0,1,10],
                4 => (20..27).chain([10]).collect(),
                _ => vec![2,6,13,14,15],
            }};
        }
        match self.section {
            0 if self.map_detail => {
                let mut rows = vec![50, 51];
                if let Some(path) = self.maps.get(self.selected_map).and_then(|m| m.path.as_deref()) {
                    rows.extend(self.destinations.iter().enumerate().filter(|(_,d)| d.matrix.is_some() && crate::teleport_menu::same_map(path, &d.map)).map(|(i,_)| 1_000_000 + i));
                }
                rows
            }
            0 => (1000..1000 + self.maps.len()).collect(),
            1 => vec![3, 8, 10],
            2 => vec![0, 1, 2, 4, 13],
            4 => vec![7, 11, 14],
            i if i >= SECTIONS.len() => self.custom_sections.get(i-SECTIONS.len()).map_or(Vec::new(), |(_,entries)| (200..200+entries.len()).collect()),
            _ => Vec::new(),
        }
    }
    fn select_section(&mut self, section: usize) {
        self.section = section;
        self.map_detail = false;
        self.multiplayer = section == 3;
        self.browser = false;
        self.network_page = 0;
        self.daylight = false;
        self.selected = self.rows().first().copied().unwrap_or(100 + section);
        self.status.clear();
    }
}

#[derive(Resource)]
struct SceneTarget(Handle<Image>);
#[derive(Resource)]
struct FramePacer(Instant);
#[derive(Component)]
struct MenuRoot;
#[derive(Component)]
pub(crate) struct MenuLayoutRoot;
#[derive(Component)]
pub(crate) struct MenuRow(usize);
#[derive(Component)]
struct MenuLabel(usize);
#[derive(Component)]
struct StatusLabel;

#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub(crate) struct MenuInput;

/// The presentation camera must exist before overlays select their UI target.
#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub(crate) struct PresentationSetup;

pub(crate) struct GraphicsMenuPlugin;
impl Plugin for GraphicsMenuPlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(FramePacer(Instant::now()))
            .init_resource::<AutoScale>()
            .add_systems(PostStartup, setup.in_set(PresentationSetup))
            .add_systems(PreUpdate, (refresh_sections, interact).chain().in_set(MenuInput).after(bevy::input::InputSystems))
            .add_systems(PreUpdate, finish_menu_travel.after(crate::map_transition::MapTransitionSet).before(crate::input::poll_controllers))
            .add_systems(PreUpdate, toggle_fullscreen.after(bevy::input::InputSystems))
            .add_systems(Update, preview_menu.before(labels))
            .add_systems(Update, (crate::map_render::advance_day, apply, labels, scroll_menu, resize_menu).chain())
            .add_systems(PostUpdate, crate::map_render::position_celestial_bodies.before(bevy::transform::TransformSystems::Propagate))
            .add_systems(Last, pace);
    }
}
fn setup(
    mut commands: Commands,
    config: Res<crate::config::Config>,
    mut images: ResMut<Assets<Image>>,
    mut window: Single<&mut Window, With<PrimaryWindow>>,
    cameras: Query<Entity, With<Camera3d>>,
    mut time: ResMut<Time<Virtual>>,
) {
    let path = config
        .asset_root
        .parent()
        .unwrap_or(&config.asset_root)
        .join("settings/graphics.json");
    let settings = match std::fs::read(&path) {
        Ok(bytes) => serde_json::from_slice::<GraphicsSettings>(&bytes).unwrap_or_else(|e| {
            warn!("Graphics settings: {e}");
            GraphicsSettings::default()
        }),
        Err(_) => GraphicsSettings::default(),
    }
    .validated();
    apply_display_mode(&mut window, &settings);
    window.present_mode = PresentMode::AutoNoVsync;
    let size = settings.internal_size(window.physical_size());
    let mut image = Image::new_target_texture(size.x, size.y, TextureFormat::Rgba8UnormSrgb, None);
    image.sampler = ImageSampler::linear();
    let target = images.add(image);
    for camera in &cameras {
        commands.entity(camera).insert((
            RenderTarget::Image(target.clone().into()),
            // Fixed policy, not a setting: see `render_capacity`.
            Msaa::Off,
        ));
        // Render the world before the presentation camera consumes its image.
        commands.entity(camera).insert(Camera {
            order: -1,
            ..default()
        });
    }
    let output = commands
        // The fullscreen scene quad covers every pixel; clearing first is a
        // wasted full-frame tile clear on TBDR.
        .spawn((Camera2d, Msaa::Off, IsDefaultUiCamera, Camera { clear_color: ClearColorConfig::None, ..default() }))
        .id();
    commands.spawn((
        Node {
            width: percent(100),
            height: percent(100),
            position_type: PositionType::Absolute,
            ..default()
        },
        ImageNode::new(target.clone()),
        UiTargetCamera(output),
    ));
    let maps = crate::map_library::discover(&config.asset_root);
    let destinations = crate::teleport_menu::load(&config.asset_root).unwrap_or_else(|e| { warn!("Map destinations: {e}"); Vec::new() });
    commands.spawn((MenuRoot, MenuLayoutRoot, UiTargetCamera(output), GlobalZIndex(10), Node {
        display: Display::None, width:percent(100), height:percent(100), align_items:AlignItems::Center,
        justify_content:JustifyContent::Center, position_type:PositionType::Absolute, ..default()
    }, BackgroundColor(Color::srgba(0.015,0.02,0.025,0.90)))).with_children(|root| {
        root.spawn((Node { width:px(1180), height:px(700), flex_shrink:0., padding:UiRect::all(px(24)), column_gap:px(28), ..default() },
            BackgroundColor(Color::srgb(0.035,0.045,0.05)))).with_children(|panel| {
            panel.spawn(Node { width:px(210), flex_shrink:0., flex_direction:FlexDirection::Column, row_gap:px(8), overflow:Overflow::scroll_y(), ..default() }).with_children(|rail| {
                rail.spawn((Text::new("SKATE / 3"),TextFont {font_size:30.,..default()},TextColor(Color::srgb(0.78,0.96,0.3))));
                rail.spawn((Text::new("OFF THE BOARD"),TextFont {font_size:12.,..default()},TextColor(Color::srgb(0.55,0.62,0.62))));
                rail.spawn(Node {height:px(24),..default()});
                for i in 0..SECTIONS.len()+8 {
                    rail.spawn((Button,MenuRow(100+i),Node {width:percent(100),min_height:px(44),padding:UiRect::all(px(12)),align_items:AlignItems::Center,..default()},BackgroundColor(Color::NONE)))
                        .with_child((MenuLabel(100+i),Text::new(""),TextFont {font_size:16.,..default()},TextColor(Color::WHITE)));
                }
                rail.spawn((Text::new("ESC / START  /  RESUME"),TextFont {font_size:12.,..default()},TextColor(Color::srgb(0.55,0.62,0.62)),Node {margin:UiRect::top(px(20)),..default()}));
            });
            panel.spawn(Node {flex_grow:1.,min_width:px(0),flex_direction:FlexDirection::Column,row_gap:px(12),..default()}).with_children(|body| {
                body.spawn((Text::new("MAPS"),MenuTitle,TextFont {font_size:40.,..default()},TextColor(Color::WHITE)));
                body.spawn((Text::new(""),MenuSubtitle,TextFont {font_size:16.,..default()},TextColor(Color::srgb(0.65,0.72,0.72))));
                body.spawn((Node {height:px(3),width:px(64),margin:UiRect::bottom(px(10)),..default()},BackgroundColor(Color::srgb(0.78,0.96,0.3))));
                body.spawn((MenuScroll,ScrollPosition::default(),Node {flex_grow:1.,min_height:px(0),overflow:Overflow::scroll_y(),flex_direction:FlexDirection::Column,row_gap:px(8),..default()})).with_children(|list| {
                    for i in (0..10).chain(11..16).chain(20..27).chain([10]).chain(200..264).chain([50,51]).chain(1000..1000+maps.len()).chain(1_000_000..1_000_000+destinations.len()) {
                        list.spawn((Button,MenuRow(i),Node {width:percent(100),min_height:px(56),flex_shrink:0.,padding:UiRect::axes(px(18),px(12)),align_items:AlignItems::Center,border_radius:BorderRadius::all(px(4)),..default()},BackgroundColor(Color::srgb(0.075,0.09,0.095))))
                            .with_child((MenuLabel(i),Text::new(""),TextFont {font_size:18.,..default()},TextColor(Color::WHITE)));
                    }
                });
                body.spawn((StatusLabel,Text::new(""),TextFont {font_size:14.,..default()},TextColor(Color::srgb(0.78,0.96,0.3))));
                body.spawn((Text::new("Up/Down Navigate    Enter / A Select    Left/Right Adjust    Tab Sections\nSettings save automatically"),TextFont {font_size:13.,..default()},TextColor(Color::srgb(0.55,0.62,0.62))));
            });
        });
    });
    commands.insert_resource(SceneTarget(target));
    let selected_map = maps.iter().position(|m| m.path.as_ref() == config.map_path.as_ref()).unwrap_or(0);
    if config.start_paused { time.pause(); }
    commands.insert_resource(Menu {
        open: config.start_paused,
        selected: 1000,
        settings,
        path,
        difficulty: config.difficulty,
        status: String::new(),
        maps,
        selected_map,
        multiplayer: false,
        browser: false, network_page: 0, browser_count: 0,
        daylight: false, section: 0, custom_sections: Vec::new(), map_detail: false, destinations, pending_travel: None,
    });
}
fn cycle<T: PartialEq + Copy>(values: &[T], value: T, direction: i32) -> T {
    let index = values.iter().position(|x| *x == value).unwrap_or(0) as i32;
    values[(index + direction).rem_euclid(values.len() as i32) as usize]
}
fn refresh_sections(mods: Res<crate::modding::Mods>, mut menu: ResMut<Menu>) {
    let mut groups = std::collections::BTreeMap::<String,Vec<(String,String,String)>>::new();
    for ((owner,key),definition) in &mods.custom_menus {
        if let Some(section)=&definition.section {
            groups.entry(section.clone()).or_default().push((owner.clone(),key.clone(),definition.title.clone()));
        }
    }
    let selected=menu.custom_sections.get(menu.section.saturating_sub(SECTIONS.len())).filter(|_|menu.section>=SECTIONS.len()).map(|(name,_)|name.clone());
    menu.custom_sections=groups.into_iter().collect();
    if let Some(name)=selected {
        if let Some(i)=menu.custom_sections.iter().position(|(n,_)|n==&name) {menu.section=SECTIONS.len()+i;} else {menu.select_section(0);}
    }
}

pub(crate) fn interact(
    mut config: ResMut<crate::config::Config>,
    mut transition: ResMut<crate::map_transition::MapTransition>,
    mut customiser: ResMut<crate::customiser::Customiser>,
    mut custom_models: ResMut<crate::custom_models::CustomModels>,
    nav: Res<crate::customiser::Navigation>,
    mut physics: ResMut<crate::physics::GamePhysics>,
    keys: Res<ButtonInput<KeyCode>>,
    mut menu: ResMut<Menu>,
    mut time: ResMut<Time<Virtual>>,
    buttons: Query<(&Interaction, &MenuRow), Changed<Interaction>>,
    mut exit: MessageWriter<AppExit>,
    mut net: ResMut<crate::multiplayer::Multiplayer>,
    mut typing: MessageReader<bevy::input::keyboard::KeyboardInput>,
    mut updater: ResMut<crate::updater::Updater>,
    travel: Res<crate::teleport_menu::Travel>,
    mut mods: ResMut<crate::modding::ModMenu>,
) {
    if transition.busy() {
        menu.open = true;
        time.pause();
        return;
    }
    if travel.open || travel.closed_this_frame || customiser.open || custom_models.open || mods.open {
        return;
    }
    if keys.just_pressed(KeyCode::Escape) || nav.pressed & 0x10 != 0 || (menu.open && nav.pressed & 0x2000 != 0) {
        if menu.open && menu.multiplayer && (menu.browser || menu.network_page != 0) {
            menu.browser = false; if menu.network_page==1 {menu.browser=true;menu.network_page=0;menu.selected=8;} else {let from_debug=menu.network_page==4;menu.network_page = 0; menu.selected = if from_debug {15} else {2};}
        } else if menu.open && menu.map_detail {
            menu.map_detail = false;
            menu.selected = 1000 + menu.selected_map;
        } else { menu.open = !menu.open; }
    }
    let mut action = None;
    for event in typing.read() {
        if !menu.open
            || !menu.multiplayer
            || menu.browser
            || !matches!(menu.selected, 3 | 7)
            || !event.state.is_pressed()
        {
            continue;
        }
        if menu.selected == 7 {
            let mut name = net.player_name.clone();
            if event.key_code == KeyCode::Backspace { name.pop(); }
            if let Some(text) = &event.text {
                for ch in text.chars().filter(|c| c.is_ascii_alphanumeric() || matches!(c, ' ' | '-' | '_')) {
                    if name.chars().count() < 16 { name.push(ch); }
                }
            }
            if name != net.player_name { net.set_player_name(name); }
            continue;
        }
        if event.key_code == KeyCode::Backspace {
            net.join_code.pop();
        }
        if let Some(text) = &event.text {
            for ch in text.chars().filter(|c| c.is_ascii_hexdigit() || *c == '-') {
                if net.join_code.len() < 40 {
                    net.join_code.push(ch);
                }
            }
        }
    }
    if menu.open {
        menu.browser_count = net.browser_rows.len();
        if keys.just_pressed(KeyCode::Tab) {
            let direction = if keys.pressed(KeyCode::ShiftLeft) || keys.pressed(KeyCode::ShiftRight) { SECTIONS.len()+menu.custom_sections.len()-1 } else { 1 };
            let section = (menu.section + direction) % (SECTIONS.len()+menu.custom_sections.len());
            menu.select_section(section);
        }
        let mut visible = menu.rows();
        visible.extend(100..100 + SECTIONS.len()+menu.custom_sections.len());
        let index = visible.iter().position(|r| *r == menu.selected).unwrap_or(0);
        let rows = visible.len();
        if keys.just_pressed(KeyCode::ArrowUp) || nav.pressed & 1 != 0 {
            menu.selected = visible[(index + rows - 1) % rows];
        }
        if keys.just_pressed(KeyCode::ArrowDown) || nav.pressed & 2 != 0 {
            menu.selected = visible[(index + 1) % rows];
        }
        let adjustable = (menu.daylight && menu.selected < 3)
            || (!menu.multiplayer
                && !menu.daylight
                && (menu.selected < 4 || (menu.section == 2 && menu.selected == 4)));
        if adjustable && (keys.just_pressed(KeyCode::ArrowLeft) || nav.pressed & 4 != 0) {
            action = Some((menu.selected, -1));
        }
        if (adjustable && (keys.just_pressed(KeyCode::ArrowRight) || nav.pressed & 8 != 0))
            || (keys.just_pressed(KeyCode::Enter)
                && !keys.pressed(KeyCode::AltLeft)
                && !keys.pressed(KeyCode::AltRight))
            || nav.pressed & 0x1000 != 0
        {
            action = Some((menu.selected, 1));
        }
        for (interaction, row) in &buttons {
            if *interaction == Interaction::Pressed && visible.contains(&row.0) {
                menu.selected = row.0;
                action = Some((row.0, 1));
            }
        }
    }
    if let Some((row, _)) = action {
        if (100..100 + SECTIONS.len()+menu.custom_sections.len()).contains(&row) {
            menu.select_section(row - 100);
            action = None;
        } else if (200..264).contains(&row) {
            if let Some((_,entries))=menu.custom_sections.get(menu.section.saturating_sub(SECTIONS.len())) {
                if let Some((owner,key,_))=entries.get(row-200) {mods.open_registered(owner.clone(),key.clone());}
            }
            action=None;
        } else if row == 50 {
            menu.map_detail = false;
            menu.selected = 1000 + menu.selected_map;
            menu.status.clear();
            action = None;
        } else if row == 51 || row >= 1_000_000 {
            if let Some(entry) = menu.maps.get(menu.selected_map).cloned() {
                let matrix = row.checked_sub(1_000_000).and_then(|i| menu.destinations.get(i)).and_then(|d| d.matrix);
                if net.active() && entry.path != config.map_path {
                    menu.status = "Leave multiplayer before switching maps".into();
                } else if let Some(matrix) = matrix {
                    menu.pending_travel = Some((entry.path.clone(), matrix));
                    if entry.path != config.map_path { transition.request(entry); }
                    menu.status = "Travelling to your spot...".into();
                } else if row == 51 {
                    if net.active() { menu.status = "Leave multiplayer before reloading a map".into(); }
                    else { transition.request(entry); menu.status = "Loading map...".into(); }
                }
            }
            action = None;
        } else if row >= 1000 {
            menu.selected_map = row - 1000;
            menu.map_detail = true;
            menu.selected = 51;
            menu.status.clear();
            action = None;
        }
    }

    if let Some((row, direction)) = action {
        let day_action = menu.daylight;
        if menu.daylight {
            match row {
                0 => menu.settings.hour = ((menu.settings.hour * 4.).round() + direction as f32).rem_euclid(96.) / 4.,
                1 => menu.settings.day_speed = cycle(DAY_SPEEDS, menu.settings.day_speed, direction),
                2 => {
                    // Auto, 0%, 5%, ... 100%, then Auto again.
                    let index = menu.settings.ambient_level.map_or(0, |level| level as i32 / 5 + 1);
                    let next = (index + direction).rem_euclid(22);
                    menu.settings.ambient_level = if next == 0 { None } else { Some((next as u32 - 1) * 5) };
                }
                _ => { menu.daylight = false; menu.selected = 13; }
            }
        } else if menu.browser {
            match row {
                0 => net.browse(0),
                1..=5 => net.join_row(row - 1),
                6 => {
                    let page = net.browser_page.saturating_sub(1);
                    net.browse(page);
                }
                7 => {
                    let page = net.browser_page + 1;
                    if page * 5 < net.browser_total {
                        net.browse(page);
                    }
                }
                8 => { menu.browser=false;menu.network_page=1;menu.selected=3; },
                9 => {
                    exit.write(AppExit::Success);
                }
                10 => {
                    menu.browser = false;
                    menu.selected = 6;
                }
                _ => {}
            }
        } else if menu.multiplayer {
            match row {
                0 => net.local(true),
                1 => net.local(false),
                2 => net.steam(true),
                3 => {}
                4 => net.steam(false),
                5 => net.leave(),
                6 => {
                    net.browse(0);
                    menu.browser = true;
                    menu.selected = 0;
                }
                12 => { menu.browser=false;menu.network_page=1;menu.selected=3; },
                8 => menu.open = false,
                9 => {
                    exit.write(AppExit::Success);
                }
                10 => {
                    if menu.network_page==1 {menu.browser=true;menu.network_page=0;menu.selected=8;} else {let from_debug=menu.network_page==4;menu.network_page = 0; menu.selected = if from_debug {15} else {2};}
                }
                13 => { menu.network_page = 2; menu.selected = 7; }
                14 => { menu.network_page = 3; menu.selected = 0; }
                15 => { menu.network_page = 4; menu.selected = 20; }
                _ => {}
            }
        } else {
            match row {
                0 => {
                    let size = cycle(
                        RESOLUTIONS,
                        (menu.settings.width, menu.settings.height),
                        direction,
                    );
                    (menu.settings.width, menu.settings.height) = size;
                }
                1 => menu.settings.scale = cycle(SCALES, menu.settings.scale, direction),
                2 => menu.settings.fps = cycle(LIMITS, menu.settings.fps, direction),
                4 => {
                    menu.settings.display_mode =
                        (menu.settings.display_mode as i32 + direction).rem_euclid(3) as u8;
                }
                3 => {
                    menu.difficulty = cycle(&Difficulty::ALL, menu.difficulty, direction);
                    physics.set_difficulty(menu.difficulty);
                    config.difficulty = menu.difficulty;
                    menu.status = match menu.difficulty.save(&config.asset_root) {
                        Ok(()) => "Difficulty saved".into(),
                        Err(e) => format!("Applied, but could not save: {e}"),
                    };
                }
                6 => menu.open = false,
                7 => {
                    exit.write(AppExit::Success);
                }
                8 => { custom_models.request_stock(); customiser.begin(); },
                9 => {
                    menu.multiplayer = true;
                    menu.selected = 0;
                }
                10 => custom_models.begin(),
                11 => menu.status = updater.open(false),
                12 => {
                    menu.select_section(0);
                    menu.selected_map = menu.maps.iter().position(|m| m.path == config.map_path).unwrap_or(0);
                    menu.map_detail = true;
                    menu.selected = 51;
                },
                13 => { menu.daylight = true; menu.selected = 0; menu.status = "Custom maps: change time, cycle speed and ambient light. Retail lighting stays authored.".into(); },
                14 => mods.begin(),
                _ => {}
            }
        }
        if ((row < 3 || row == 4) && !menu.multiplayer && !menu.daylight && !day_action) || (day_action && row < 3) {
            let save = save_settings(&menu);
            menu.status = match save {
                Ok(()) => "Saved".into(),
                Err(e) => format!("Could not save: {e}"),
            };
        }
    }
    if menu.open && !net.active() {
        time.pause();
    } else {
        time.unpause();
    }
}
fn toggle_fullscreen(
    keys: Res<ButtonInput<KeyCode>>,
    mut menu: ResMut<Menu>,
    mut window: Single<&mut Window, With<PrimaryWindow>>,
) {
    let alt = keys.pressed(KeyCode::AltLeft) || keys.pressed(KeyCode::AltRight);
    if !alt || !keys.just_pressed(KeyCode::Enter) {
        return;
    }
    match window.mode {
        WindowMode::Windowed => {
            window.mode = WindowMode::BorderlessFullscreen(MonitorSelection::Primary);
            menu.settings.display_mode = 1;
        }
        WindowMode::BorderlessFullscreen(_) | WindowMode::Fullscreen(_, _) => {
            window.mode = WindowMode::Windowed;
            window
                .resolution
                .set_physical_resolution(menu.settings.width, menu.settings.height);
            menu.settings.display_mode = 0;
        }
    }
    menu.status = match save_settings(&menu) {
        Ok(()) => "Saved".into(),
        Err(e) => format!("Could not save: {e}"),
    };
}
/// Apply the saved display mode to the window. Exclusive fullscreen keeps the
/// monitor's current video mode; returning to windowed restores the saved size.
fn apply_display_mode(window: &mut Window, settings: &GraphicsSettings) {
    match settings.display_mode {
        2 => {
            window.mode =
                WindowMode::Fullscreen(MonitorSelection::Primary, VideoModeSelection::Current);
        }
        1 => {
            window.mode = WindowMode::BorderlessFullscreen(MonitorSelection::Primary);
        }
        _ => {
            window.mode = WindowMode::Windowed;
            window
                .resolution
                .set_physical_resolution(settings.width, settings.height);
        }
    }
}
fn save_settings(menu: &Menu) -> Result<(), String> {
    std::fs::create_dir_all(menu.path.parent().unwrap()).map_err(|e| e.to_string())?;
    std::fs::write(
        &menu.path,
        serde_json::to_vec_pretty(&menu.settings).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())
}
fn apply(
    menu: Res<Menu>,
    mut window: Single<&mut Window, With<PrimaryWindow>>,
    target: Res<SceneTarget>,
    mut images: ResMut<Assets<Image>>,
    mut previous: Local<Option<GraphicsSettings>>,
) {
    if previous.as_ref().is_none_or(|p| {
        p.display_mode != menu.settings.display_mode
            || p.width != menu.settings.width
            || p.height != menu.settings.height
    }) {
        apply_display_mode(&mut window, &menu.settings);
    }
    let size = menu.settings.internal_size(window.physical_size());
    if let Some(image) = images.get(&target.0) {
        if image.size() != size {
            images.get_mut(&target.0).unwrap().resize(Extent3d {
                width: size.x,
                height: size.y,
                depth_or_array_layers: 1,
            });
        }
    }
    *previous = Some(menu.settings.clone());
}
fn labels(
    menu: Res<Menu>,
    transition: Res<crate::map_transition::MapTransition>,
    time: Res<Time<Real>>,
    customiser: Res<crate::customiser::Customiser>,
    custom_models: Res<crate::custom_models::CustomModels>,
    travel: Res<crate::teleport_menu::Travel>,
    mods: Res<crate::modding::ModMenu>,
    net: Res<crate::multiplayer::Multiplayer>,
    window: Single<&Window, With<PrimaryWindow>>,
    mut root: Single<&mut Node, With<MenuRoot>>,
    mut labels: Query<(&MenuLabel, &mut Text), (Without<StatusLabel>, Without<MenuTitle>, Without<MenuSubtitle>)>,
    mut headings: Query<(&mut Text, Has<MenuTitle>), (Or<(With<MenuTitle>, With<MenuSubtitle>)>, Without<StatusLabel>)>,
    mut status: Single<&mut Text, With<StatusLabel>>,
    debug: (Res<crate::modding::Mods>, Res<crate::physics::GamePhysics>, Res<crate::multiplayer::appearance::Appearances>),
    mut buttons: Query<(&MenuRow, &Interaction, &mut BackgroundColor, &mut Node), Without<MenuRoot>>,
    auto_scale: Option<Res<AutoScale>>,
) {
    root.display = if menu.open && !travel.open && !customiser.open && !custom_models.open && !mods.open {
        Display::Flex
    } else {
        Display::None
    };
    if !menu.open {
        return;
    }
    for (mut text, title) in &mut headings {
        **text = if menu.map_detail {
            if title { menu.maps.get(menu.selected_map).map_or("MAP", |m| m.label.as_str()) }
            else { "Choose a teleport spot, or skate from the default spawn." }
        } else if menu.daylight {
            if title { "DAY & NIGHT" } else { "Custom maps: time and ambient light. Retail lighting stays authored." }
        } else if menu.browser {
            if title { "FIND A SESSION" } else { "Browse public lobbies and join a crew." }
        } else if menu.multiplayer && menu.network_page != 0 {
            match (menu.network_page, title) {
                (1,true) => "JOIN A FRIEND", (1,false) => "Select the code field, type your friend's code, then choose Join session.",
                (2,true) => "PLAYER & SESSION", (2,false) => "Select your name to edit it. Leave your current session here.",
                (4,true) => "MULTIPLAYER DEBUG", (4,false) => "Live network and mod diagnostics. Scroll or use Up/Down to inspect.",
                (3,true) => "LOCAL TESTING", (_,false) => "Advanced: host or join a local test session without Steam.",
                _ => "MULTIPLAYER",
            }
        } else if title { SECTIONS.get(menu.section).map_or_else(||menu.custom_sections.get(menu.section-SECTIONS.len()).map_or("",|(n,_)|n.as_str()),|s|s.0) } else { SECTIONS.get(menu.section).map_or("Choose an activity.",|s|s.1) }.into();
    }
    let mut debug_rows = Vec::new();
    if menu.multiplayer && menu.network_page == 4 {
        debug_rows.extend(net.debug_sections());
        debug_rows.push(format!("CHARACTERS & COLLISIONS\n{} {}\nPlayer contacts: {} | Network active: {}",
            debug.2.progress, debug.2.status, debug.1.network_contacts, debug.1.network_active));
        debug_rows.extend(debug.0.multiplayer_debug_sections());
    }
    let s = &menu.settings;
    let effective = auto_scale.as_ref().map(|a| a.effective(s.scale)).unwrap_or(s.scale);
    let size = GraphicsSettings::scaled_size(window.physical_size(), effective);
    for (label, mut text) in &mut labels {
        **text = if (100..113).contains(&label.0) {
            let i=label.0-100;
            let name=SECTIONS.get(i).map(|s|s.0).or_else(||menu.custom_sections.get(i.saturating_sub(SECTIONS.len())).map(|(n,_)|n.as_str())).unwrap_or("");
            format!("{:02}   {}",i+1,name)
        } else if (200..264).contains(&label.0) {
            menu.custom_sections.get(menu.section.saturating_sub(SECTIONS.len())).and_then(|(_,e)|e.get(label.0-200)).map(|(_,_,t)|t.clone()).unwrap_or_default()
        } else if label.0 >= 1_000_000 {
            menu.destinations.get(label.0 - 1_000_000).map(|d| d.name.clone()).unwrap_or_default()
        } else if label.0 == 50 { "<  All maps".into()
        } else if label.0 == 51 { "Skate from default spawn".into()
        } else if label.0 >= 1000 {
            menu.maps.get(label.0 - 1000).map(|entry| format!("{}    /    VIEW SPOTS", entry.label)).unwrap_or_default()
        } else if menu.multiplayer && menu.network_page == 4 && (20..27).contains(&label.0) {
            debug_rows.get(label.0 - 20).cloned().unwrap_or_default()
        } else if menu.daylight {
            match label.0 {
                0 => { let minutes = (s.hour * 60.).floor() as u32 % 1440; format!("Time of day          {:02}:{:02}", minutes / 60, minutes % 60) },
                1 => if s.day_speed == 0 { "Cycle speed          Frozen".into() } else { format!("Cycle speed          {}x ({} min/day)", s.day_speed, 1440 / s.day_speed) },
                2 => match s.ambient_level {
                    Some(level) => format!("Ambient light        {level}%"),
                    None => "Ambient light        Auto (day/night)".into(),
                },
                3 => "Back".into(),
                _ => String::new(),
            }
        } else if menu.browser {
            match label.0 {
                0 => "Refresh sessions".into(),
                1..=5 => net
                    .browser_rows
                    .get(label.0 - 1)
                    .map(|r| {
                        format!(
                            "{} | {}/{} | #{}{}",
                            r.map,
                            r.players,
                            r.capacity,
                            r.id % 100000,
                            if r.compatible {
                                ""
                            } else {
                                " | Update required"
                            }
                        )
                    })
                    .unwrap_or_else(|| "--".into()),
                6 => "Previous page".into(),
                7 => "Next page".into(),
                8 => "Join with a code".into(),
                9 => "Quit game".into(),
                _ => "Back to multiplayer".into(),
            }
        } else if menu.multiplayer {
            match label.0 {
                0 => "Host local session".into(),
                1 => "Join local session".into(),
                2 => "Host a session".into(),
                3 => format!(
                    "Join code: {}{}",
                    net.join_code,
                    if menu.selected == 3 { "_" } else { "" }
                ),
                4 => "Join session".into(),
                5 => "Leave multiplayer".into(),
                6 => "Find a session".into(),
                7 => format!("Your name: {}{}", net.player_name, if menu.selected == 7 { "_" } else { "" }),
                12 => "Join with a code".into(),
                8 => "Resume".into(),
                9 => "Quit game".into(),
                13 => "Player & session".into(),
                14 => "Advanced / local testing".into(),
                15 => "Debug".into(),
                _ => "<  Multiplayer".into(),
            }
        } else {
            match label.0 {
                0 => format!("Resolution          {} x {}", s.width, s.height),
                1 => {
                    if effective == s.scale {
                        format!("Internal resolution   {}%  ({} x {})", s.scale, size.x, size.y)
                    } else {
                        format!("Internal resolution   {}%  ({} x {})  [auto {}%]", s.scale, size.x, size.y, effective)
                    }
                }
                2 => format!(
                    "FPS limit             {}",
                    if s.fps == 0 {
                        "Unlimited".into()
                    } else {
                        s.fps.to_string()
                    }
                ),
                4 => format!(
                    "Display mode          {}",
                    DISPLAY_MODES
                        .get(s.display_mode as usize)
                        .unwrap_or(&"Windowed")
                ),
                3 => format!("Difficulty            {}", menu.difficulty.label()),
                6 => "Resume".into(),
                7 => "Quit game".into(),
                8 => "Character customiser".into(),
                10 => "Custom models".into(),
                11 => "Updates".into(),
                12 => "Teleport".into(),
                13 => "Day & night".into(),
                14 => "Mods".into(),
                _ => "Multiplayer".into(),
            }
        };
    }
    ***status = if transition.busy() {
        format!("{} {}\nGameplay is paused. Please wait.", ["|", "/", "-", "\\"][(time.elapsed_secs() * 4.) as usize % 4], transition.label())
    } else if menu.multiplayer && menu.network_page == 4 {
        "Diagnostics stay in this menu; gameplay shows names and ping only.".into()
    } else if menu.browser {
        net.browser_status.clone()
    } else if menu.multiplayer {
        format!(
            "{}{}",
            net.status,
            if net.host_code.is_empty() {
                String::new()
            } else {
                format!("\nYour connection code: {}", net.host_code)
            }
        )
    } else {
        menu.status.clone()
    };
    let visible = menu.rows();
    for (row, interaction, mut color, mut node) in &mut buttons {
        node.display = if visible.contains(&row.0) || (100..100+SECTIONS.len()+menu.custom_sections.len()).contains(&row.0) { Display::Flex } else { Display::None };
        color.0 = if row.0 == menu.selected || row.0 == 100 + menu.section || *interaction == Interaction::Hovered {
            Color::srgb(0.24, 0.33, 0.12)
        } else {
            Color::srgb(0.075, 0.09, 0.095)
        };
    }
}
// Opt-in screenshot coverage for overlays; never changes an ordinary session.
fn preview_menu(config: Res<crate::config::Config>, mut menu: ResMut<Menu>, mut mods: ResMut<crate::modding::ModMenu>, mut done: Local<bool>) {
    if *done || config.verification_capture.is_none() { return; }
    *done = true;
    match std::env::var("SKATE_VERIFY_MENU").as_deref() {
        Ok("mods") => { menu.open = true; mods.begin(); }
        Ok("multiplayer") => { menu.open = true; menu.select_section(3); }
        _ => {}
    }
}
fn finish_menu_travel(
    mut menu: ResMut<Menu>, transition: Res<crate::map_transition::MapTransition>,
    current: Res<crate::map_transition::CurrentMap>, mut skater: ResMut<crate::physics::SkaterRuntime>,
    mut time: ResMut<Time<Virtual>>,
) {
    if transition.busy() { return; }
    let Some((path, matrix)) = menu.pending_travel.take() else { return; };
    // A failed map transaction retains the previous world: never apply another map's coordinates there.
    if current.path != path { return; }
    match skater.travel_to(matrix) {
        Ok(()) => { menu.open = false; time.unpause(); }
        Err(e) => { menu.open = true; menu.status = format!("Could not travel: {e}"); }
    }
}
fn resize_menu(
    window: Single<&Window, With<PrimaryWindow>>, roots: Query<Entity, With<MenuLayoutRoot>>,
    children: Query<&Children>, mut nodes: Query<(&mut Node, Option<&mut TextFont>)>,
    mut previous: Local<Option<f32>>,
) {
    let scale = (window.width() / 1280.).min(window.height() / 800.).max(0.25);
    let factor = scale / previous.unwrap_or(1.);
    if (factor - 1.).abs() < 0.0001 { return; }
    fn resize(value: &mut Val, factor: f32) { if let Val::Px(px) = value { *px *= factor; } }
    for root in &roots {
        for entity in children.iter_descendants(root) {
            if let Ok((mut node, font)) = nodes.get_mut(entity) {
                let node = &mut *node;
                for value in [&mut node.width, &mut node.height, &mut node.min_width, &mut node.min_height,
                    &mut node.max_width, &mut node.max_height, &mut node.row_gap, &mut node.column_gap,
                    &mut node.padding.left, &mut node.padding.right, &mut node.padding.top, &mut node.padding.bottom,
                    &mut node.margin.left, &mut node.margin.right, &mut node.margin.top, &mut node.margin.bottom] {
                    resize(value, factor);
                }
                if let Some(mut font) = font { font.font_size *= factor; }
            }
        }
    }
    *previous = Some(scale);
}
fn scroll_menu(
    menu: Res<Menu>,
    mut wheel: MessageReader<bevy::input::mouse::MouseWheel>,
    mut scroll: Query<(&mut ScrollPosition, &ComputedNode, &Node), With<MenuScroll>>,
    rows: Query<(&MenuRow, &ComputedNode)>,
    mut previous: Local<Option<(usize, bool, bool, usize, bool, u8)>>,
) {
    let delta: f32 = wheel.read().map(|e| e.y * if e.unit == bevy::input::mouse::MouseScrollUnit::Line { 40. } else { 1. }).sum();
    if !menu.open { return; }
    let state = (menu.section, menu.daylight, menu.browser, menu.selected, menu.map_detail, menu.network_page);
    let visible = menu.rows();
    for (mut pos, node, layout) in &mut scroll {
        let scale = node.inverse_scale_factor();
        let height = node.size().y * scale;
        let max = (node.content_size().y * scale - height).max(0.);
        if previous.as_ref().is_none_or(|p| (p.0,p.1,p.2,p.4,p.5) != (state.0,state.1,state.2,state.4,state.5)) { pos.y = 0.; }
        else if previous.as_ref() != Some(&state) {
            let mut top = 0.;
            for id in &visible {
                let row_height = rows.iter().find(|(r,_)| r.0 == *id).map_or(56., |(_,n)| n.size().y * scale);
                if *id == menu.selected {
                    if top < pos.y { pos.y = top; }
                    else if top + row_height > pos.y + height { pos.y = top + row_height - height; }
                    break;
                }
                top += row_height + if let Val::Px(gap) = layout.row_gap { gap } else { 0. };
            }
        }
        pos.y = (pos.y - delta).clamp(0., max);
    }
    *previous = Some(state);
}
fn pace(menu: Option<Res<Menu>>, mut pacer: ResMut<FramePacer>) {
    let Some(menu) = menu else {
        return;
    };
    if menu.settings.fps > 0 {
        let period = Duration::from_secs_f64(1. / f64::from(menu.settings.fps));
        if let Some(wait) = period.checked_sub(pacer.0.elapsed()) {
            std::thread::sleep(wait);
        }
    }
    pacer.0 = Instant::now();
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn culling_can_toggle_with_msaa_and_render_scale_changes() {
        let mut app = App::new();
        let mut images = Assets::<Image>::default();
        let target = images.add(Image::new_target_texture(
            1280,
            800,
            TextureFormat::Rgba8UnormSrgb,
            None,
        ));
        app.insert_resource(SceneTarget(target.clone()))
            .insert_resource(images)
            .insert_resource(Menu {
                open: false, selected: 0, settings: GraphicsSettings::default(),
                difficulty: Difficulty::Easy, path: PathBuf::new(), status: String::new(),
                multiplayer: false, browser: false, network_page: 0, browser_count: 0, daylight: false, section: 0, custom_sections: Vec::new(), map_detail: false, destinations: Vec::new(), pending_travel: None,
                maps: vec![crate::map_library::Entry { label: "Test world".into(), path: None }], selected_map: 0,
            })
            .add_systems(Update, apply);
        {
            let mut menu = app.world_mut().resource_mut::<Menu>();
            menu.settings.hour = 23.5;
            menu.settings.day_speed = 60;
            assert!((menu.advance_day(60.) - 0.5).abs() < 0.0001);
            menu.open = true;
            assert_eq!(menu.advance_day(60.), 0.5);
            menu.open = false;
            menu.settings.day_speed = 0;
            assert_eq!(menu.advance_day(60.), 0.5);
        }
        app.world_mut().spawn((Window::default(), PrimaryWindow));
        let camera = app.world_mut().spawn((Camera3d::default(), Msaa::Off)).id();
        app.update();
        // MSAA and occlusion culling are no longer settings, so the only thing
        // `apply` still adapts is the internal render target size.
        {
            let mut menu = app.world_mut().resource_mut::<Menu>();
            menu.settings.scale = 67;
        }
        app.update();
        assert_eq!(*app.world().get::<Msaa>(camera).unwrap(), Msaa::Off);
        assert_eq!(
            app.world()
                .resource::<Assets<Image>>()
                .get(&target)
                .unwrap()
                .size(),
            UVec2::new(857, 536)
        );
    }
    #[test]
    fn sections_expose_only_real_rows_and_all_maps() {
        let mut menu = Menu {
            open: true, selected: 1000, settings: GraphicsSettings::default(),
            path: PathBuf::new(), difficulty: Difficulty::Easy, status: String::new(),
            maps: (0..40).map(|i| crate::map_library::Entry { label: format!("Map {i}"), path: None }).collect(),
            selected_map: 0, multiplayer: false, browser: false, network_page: 0, browser_count: 0, daylight: false, section: 0, custom_sections: Vec::new(), map_detail: false, destinations: Vec::new(), pending_travel: None,
        };
        for section in 0..SECTIONS.len() {
            menu.select_section(section);
            let rows = menu.rows();
            assert!(rows.contains(&menu.selected));
            assert!(rows.windows(2).all(|pair| pair[0] < pair[1]));
            assert!(rows.iter().all(|id| *id < 16 || *id >= 1000));
        }
        menu.select_section(1);
        assert_eq!(menu.rows(),vec![3,8,10]);
        menu.custom_sections=vec![("Challenges".into(),vec![("test.mod".into(),"race".into(),"Race".into())])];
        menu.select_section(SECTIONS.len());
        assert_eq!(menu.rows(),vec![200]);
        menu.select_section(0);
        assert_eq!(menu.rows(), (1000..1040).collect::<Vec<_>>());
        menu.maps[0].path = Some(PathBuf::from("University.skate"));
        menu.destinations = vec![
            crate::teleport_menu::Destination { id: "uni".into(), name: "Campus".into(), map: "University".into(), matrix: Some([[0.;4];4]) },
            crate::teleport_menu::Destination { id: "dt".into(), name: "Downtown".into(), map: "DownTown".into(), matrix: Some([[0.;4];4]) },
        ];
        menu.map_detail = true;
        assert_eq!(menu.rows(), vec![50,51,1_000_000]);
        menu.select_section(0);
        menu.maps.clear();
        menu.select_section(0);
        assert_eq!(menu.selected, 100);
        assert!(menu.rows().is_empty());
        menu.select_section(3);
        assert!(menu.multiplayer);
        assert_eq!(menu.rows(), vec![2,6,13,14,15]);
        menu.network_page = 4;
        assert_eq!(menu.rows(), (20..27).chain([10]).collect::<Vec<_>>());
        menu.network_page = 1;
        assert_eq!(menu.rows(), vec![3,4,10]);
        menu.network_page = 3;
        assert_eq!(menu.rows(), vec![0,1,10]);
        menu.browser = true;
        assert!(menu.rows().contains(&8));
        assert!(!SECTIONS.iter().any(|(name,_)| matches!(*name,"SESSION"|"WORLD")));
        menu.select_section(2);
        assert!(!menu.multiplayer && !menu.browser);
        assert_eq!(menu.rows(), vec![0, 1, 2, 4, 13]);
        menu.daylight = true;
        assert_eq!(menu.rows(), vec![0,1,2,3]);
    }
    #[test]
    fn invalid_saved_values_fall_back() {
        let settings: GraphicsSettings =
            serde_json::from_str(r#"{"width":0,"height":999999,"scale":0,"fps":1}"#)
                .unwrap();
        assert_eq!(settings.validated(), GraphicsSettings::default());
        let settings: GraphicsSettings =
            serde_json::from_str(r#"{"display_mode":9}"#).unwrap();
        assert_eq!(settings.validated(), GraphicsSettings::default());
    }
    #[test]
    fn display_mode_cycles_and_applies() {
        assert_eq!(DISPLAY_MODES, &["Windowed", "Borderless", "Fullscreen"]);
        let mut settings = GraphicsSettings::default();
        assert_eq!(settings.display_mode, 0);
        for (mode, _) in DISPLAY_MODES.iter().enumerate() {
            settings.display_mode = mode as u8;
            let mut window = Window::default();
            apply_display_mode(&mut window, &settings);
            match mode {
                0 => {
                    assert_eq!(window.mode, WindowMode::Windowed);
                    assert_eq!(
                        (window.physical_width(), window.physical_height()),
                        (settings.width, settings.height)
                    );
                }
                1 => assert!(matches!(
                    window.mode,
                    WindowMode::BorderlessFullscreen(MonitorSelection::Primary)
                )),
                _ => assert!(matches!(
                    window.mode,
                    WindowMode::Fullscreen(
                        MonitorSelection::Primary,
                        VideoModeSelection::Current
                    )
                )),
            }
        }
        // Manual cycling wraps in both directions.
        assert_eq!((0i32 + 1).rem_euclid(3), 1);
        assert_eq!((0i32 - 1).rem_euclid(3), 2);
    }
    #[test]
    fn scaled_target_and_cycle_boundaries() {
        let s = GraphicsSettings {
            scale: 50,
            ..default()
        };
        assert_eq!(
            s.internal_size(UVec2::new(1920, 1080)),
            UVec2::new(960, 540)
        );
        assert_eq!(s.internal_size(UVec2::ZERO), UVec2::ONE);
        assert_eq!(cycle(LIMITS, 0, -1), 240);
        assert_eq!(cycle(LIMITS, 240, 1), 0);
    }
}

#[cfg(test)]
mod auto_scale_tests {
    use super::*;
    fn settled(user: u32) -> AutoScale {
        let mut auto = AutoScale { warmup: 0, ..AutoScale::default() };
        auto.update(Some(16.7), user, false);
        assert_eq!(auto.effective(user), user);
        auto
    }
    #[test]
    fn steady_load_holds_maximum_pixels() {
        let mut auto = settled(100);
        for _ in 0..2000 {
            assert_eq!(auto.update(Some(16.7), 100, false), None);
        }
        assert_eq!(auto.effective(100), 100);
    }
    #[test]
    fn sustained_pressure_steps_down_and_floors_at_fifty() {
        let mut auto = settled(100);
        let mut stepped = None;
        for _ in 0..400 {
            if let Some(scale) = auto.update(Some(40.0), 100, false) {
                stepped = Some(scale);
                break;
            }
        }
        assert_eq!(stepped, Some(85));
        assert_eq!(auto.effective(100), 85);
        for _ in 0..2000 {
            auto.update(Some(40.0), 100, false);
        }
        assert_eq!(auto.effective(100), 50);
        for _ in 0..2000 {
            assert_eq!(auto.update(Some(40.0), 100, false), None);
        }
        assert_eq!(auto.effective(100), 50);
    }
    #[test]
    fn headroom_recovers_stepwise_without_exceeding_the_setting() {
        let mut auto = settled(85);
        auto.effective = 50;
        let mut stepped = None;
        for _ in 0..1500 {
            if let Some(scale) = auto.update(Some(8.0), 85, false) {
                stepped = Some(scale);
                break;
            }
        }
        assert_eq!(stepped, Some(67));
        for _ in 0..2500 {
            auto.update(Some(8.0), 85, false);
        }
        assert_eq!(auto.effective(85), 85);
        for _ in 0..2000 {
            assert_eq!(auto.update(Some(8.0), 85, false), None);
        }
        assert_eq!(auto.effective(85), 85);
    }
    #[test]
    fn oscillation_around_thresholds_never_steps() {
        let mut auto = settled(100);
        for _ in 0..2000 {
            auto.update(Some(25.0), 100, false);
            auto.update(Some(10.0), 100, false);
        }
        assert_eq!(auto.effective(100), 100);
    }
    #[test]
    fn loads_warmup_and_bad_clocks_only_adopt() {
        let mut auto = AutoScale::default();
        assert_eq!(auto.update(None, 100, false), None);
        assert_eq!(auto.effective(100), 100);
        assert_eq!(auto.update(Some(f32::NAN), 100, false), None);
        assert_eq!(auto.update(Some(f32::INFINITY), 100, false), None);
        let mut loading = settled(100);
        for _ in 0..2000 {
            assert_eq!(loading.update(Some(200.0), 100, true), None);
        }
        assert_eq!(loading.effective(100), 100);
        let mut cold = AutoScale::default();
        for _ in 0..500 {
            assert_eq!(cold.update(Some(200.0), 100, false), None);
        }
        assert_eq!(cold.effective(100), 100);
    }
    #[test]
    fn user_ceiling_below_floor_never_moves() {
        let mut auto = settled(25);
        for _ in 0..2000 {
            assert_eq!(auto.update(Some(40.0), 25, false), None);
        }
        assert_eq!(auto.effective(25), 25);
    }
    #[test]
    fn manual_changes_are_adopted_immediately() {
        let mut auto = settled(100);
        auto.effective = 67;
        assert_eq!(auto.update(Some(16.7), 75, false), None);
        assert_eq!(auto.effective(75), 75);
        assert_eq!(auto.update(Some(16.7), 75, false), None);
    }
}