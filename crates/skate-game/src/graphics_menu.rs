//! Native-resolution pause UI over a separately scaled 3D render target.
use bevy::{
    camera::RenderTarget,
    core_pipeline::prepass::DepthPrepass,
    image::ImageSampler,
    prelude::*,
    render::{
        experimental::occlusion_culling::OcclusionCulling,
        render_resource::{Extent3d, TextureFormat},
        renderer::RenderAdapter,
    },
    window::{PresentMode, PrimaryWindow},
};
use serde::{Deserialize, Serialize};
use crate::difficulty::Difficulty;
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
const LIMITS: &[u32] = &[0, 30, 60, 90, 120, 144, 165, 240];

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
struct GraphicsSettings {
    width: u32,
    height: u32,
    scale: u32,
    samples: u32,
    fps: u32,
    occlusion: bool,
}
impl Default for GraphicsSettings {
    fn default() -> Self {
        Self {
            width: 1280,
            height: 800,
            scale: 100,
            samples: 4,
            fps: 0,
            occlusion: true,
        }
    }
}
impl GraphicsSettings {
    fn validated(mut self) -> Self {
        if !RESOLUTIONS.contains(&(self.width, self.height)) {
            (self.width, self.height) = (1280, 800);
        }
        if !SCALES.contains(&self.scale) {
            self.scale = 100;
        }
        if ![1, 2, 4, 8].contains(&self.samples) {
            self.samples = 4;
        }
        if !LIMITS.contains(&self.fps) {
            self.fps = 0;
        }
        self
    }
    fn internal_size(&self, window: UVec2) -> UVec2 {
        (window * self.scale / 100).max(UVec2::ONE)
    }
}
#[derive(Resource)]
pub(crate) struct Menu {
    open: bool,
    selected: usize,
    settings: GraphicsSettings,
    path: PathBuf,
    supported_msaa: Vec<u32>,
    difficulty: Difficulty,
    status: String,
    maps: Vec<crate::map_library::Entry>,
    selected_map: usize,
    destinations: Vec<crate::teleport_menu::Destination>,
    travel_open: bool,
}
pub(crate) fn gameplay_active(menu: Option<Res<Menu>>) -> bool {
    menu.is_none_or(|m| !m.open)
}
#[derive(Resource)]
struct SceneTarget(Handle<Image>);
#[derive(Resource)]
struct FramePacer(Instant);
#[derive(Component)]
struct MenuRoot;
#[derive(Component)]
struct MenuRow(usize);
#[derive(Component)]
struct MenuLabel(usize);
#[derive(Component)]
struct StatusLabel;
#[derive(Component)]
struct TravelPanel;
#[derive(Component)]
struct TravelRow(usize);
#[derive(Component)]
struct TravelViewport;
#[derive(Component)]
struct TravelTrack;
#[derive(Component)]
struct TravelThumb;
const TRAVEL_HEIGHT: f32 = 294.;
const TRAVEL_ROW: f32 = 42.;

pub(crate) struct GraphicsMenuPlugin;
impl Plugin for GraphicsMenuPlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(FramePacer(Instant::now()))
            .add_systems(PostStartup, setup)
            .add_systems(Update, (interact, apply, labels, travel_scroll).chain())
            .add_systems(Last, pace);
    }
}
fn setup(
    mut commands: Commands,
    config: Res<crate::config::Config>,
    mut images: ResMut<Assets<Image>>,
    mut window: Single<&mut Window, With<PrimaryWindow>>,
    cameras: Query<Entity, With<Camera3d>>,
    adapter: Res<RenderAdapter>,
    mut skater: ResMut<crate::physics::SkaterRuntime>,
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
    let supported_msaa: Vec<_> = [1, 2, 4, 8]
        .into_iter()
        .filter(|&samples| {
            [
                TextureFormat::Rgba16Float,
                TextureFormat::Rgba8UnormSrgb,
                TextureFormat::Depth32Float,
            ]
            .into_iter()
            .all(|format| {
                adapter
                    .get_texture_format_features(format)
                    .flags
                    .sample_count_supported(samples)
            })
        })
        .collect();
    let mut settings = settings;
    // Reproducible A/B override; normal launches use the saved menu setting.
    match std::env::var("SKATE_OCCLUSION").as_deref() {
        Ok("0") => settings.occlusion = false,
        Ok("1") => settings.occlusion = true,
        _ => {}
    }
    if !supported_msaa.contains(&settings.samples) {
        settings.samples = 1;
    }
    window
        .resolution
        .set_physical_resolution(settings.width, settings.height);
    window.present_mode = PresentMode::AutoNoVsync;
    let size = settings.internal_size(window.physical_size());
    let mut image = Image::new_target_texture(size.x, size.y, TextureFormat::Rgba8UnormSrgb, None);
    image.sampler = ImageSampler::linear();
    let target = images.add(image);
    for camera in &cameras {
        commands.entity(camera).insert((
            RenderTarget::Image(target.clone().into()),
            msaa(settings.samples),
        ));
        // Render the world before the presentation camera consumes its image.
        commands.entity(camera).insert(Camera {
            order: -1,
            ..default()
        });
    }
    let output = commands
        .spawn((Camera2d, Msaa::Off, IsDefaultUiCamera))
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
    let selected_map = maps.iter().position(|m| m.path.as_ref() == config.map_path.as_ref()).unwrap_or(0);
    let (mut destinations, mut status) = match crate::teleport_menu::load(&config.asset_root) {
        Ok(locations) => (locations, String::new()),
        Err(e) => (Vec::new(), e),
    };
    if let Some(id) = &config.teleport {
        if let Some(target) = destinations.iter().find(|d| &d.id == id).and_then(|d| d.matrix) {
            if let Err(e) = skater.travel_to(target) { status = e; }
        }
    }
    destinations.retain(|d| d.matrix.is_some() && config.map_path.as_ref()
        .is_some_and(|p| crate::teleport_menu::same_map(p, &d.map)));
    commands.spawn((MenuRoot, UiTargetCamera(output), GlobalZIndex(10), Node {
        display: Display::None, width:percent(100), height:percent(100), align_items:AlignItems::Center,
        justify_content:JustifyContent::Center, position_type:PositionType::Absolute, ..default()
    }, BackgroundColor(Color::srgba(0.015,0.025,0.04,0.88)))).with_children(|root| {
        root.spawn((Node { width:px(720),max_width:percent(95),padding:UiRect::all(px(18)),flex_direction:FlexDirection::Column,row_gap:px(6),border_radius:BorderRadius::all(px(12)),..default() },
            BackgroundColor(Color::srgb(0.035,0.055,0.08)))).with_children(|panel| {
            panel.spawn((MenuLabel(usize::MAX),Text::new("PAUSED"),TextFont {font_size:32.,..default()},TextColor(Color::WHITE)));
            panel.spawn((MenuLabel(usize::MAX-1),Text::new("GAMEPLAY & GRAPHICS"),TextFont {font_size:16.,..default()},TextColor(Color::srgb(0.4,0.85,0.85))));
            for i in 0..11 {
                panel.spawn((Button, MenuRow(i), Node {width:percent(100),min_height:px(36),padding:UiRect::all(px(8)),align_items:AlignItems::Center,border_radius:BorderRadius::all(px(5)),..default()},
                    BackgroundColor(Color::srgb(0.08,0.11,0.15)))).with_children(|row| {
                    row.spawn((MenuLabel(i),Text::new(""),TextFont {font_size:18.,..default()},TextColor(Color::WHITE)));
                });
            }
            panel.spawn((TravelPanel, Node { display: Display::None, flex_direction: FlexDirection::Column, row_gap:px(6), ..default() })).with_children(|travel| {
                travel.spawn(Node { height:px(TRAVEL_HEIGHT), column_gap:px(8), ..default() }).with_children(|list| {
                    list.spawn((TravelViewport, ScrollPosition::default(), Node {
                        flex_grow:1., flex_basis:px(0), height:px(TRAVEL_HEIGHT), overflow:Overflow::scroll_y(),
                        flex_direction:FlexDirection::Column, ..default()
                    })).with_children(|rows| {
                        for (i, destination) in destinations.iter().enumerate() {
                            rows.spawn((Button, TravelRow(i), Node { width:percent(100), height:px(TRAVEL_ROW), flex_shrink:0., padding:UiRect::horizontal(px(8)), align_items:AlignItems::Center, ..default() }, BackgroundColor(Color::srgb(0.08,0.11,0.15))))
                                .with_child((Text::new(&destination.name), TextFont {font_size:18.,..default()}, TextColor(Color::WHITE)));
                        }
                        if destinations.is_empty() {
                            rows.spawn((Text::new("No teleport destinations available on this map."), TextFont {font_size:18.,..default()}, TextColor(Color::WHITE)));
                        }
                    });
                    list.spawn((TravelTrack, Button, bevy::ui::RelativeCursorPosition::default(), Node { width:px(16), height:percent(100), ..default() }, BackgroundColor(Color::srgb(0.02,0.035,0.05))))
                        .with_child((TravelThumb, bevy::ui::FocusPolicy::Pass, Node { position_type:PositionType::Absolute, width:percent(100), height:percent(100), border_radius:BorderRadius::all(px(6)), ..default() }, BackgroundColor(Color::srgb(0.3,0.65,0.68))));
                });
                travel.spawn((Button, TravelRow(destinations.len()), Node {height:px(TRAVEL_ROW),padding:UiRect::horizontal(px(8)),align_items:AlignItems::Center,..default()},BackgroundColor(Color::srgb(0.08,0.11,0.15))))
                    .with_child((Text::new("Back to pause menu"),TextFont {font_size:18.,..default()},TextColor(Color::WHITE)));
            });
            panel.spawn((StatusLabel,Text::new(""),TextFont {font_size:15.,..default()},TextColor(Color::srgb(0.65,0.75,0.8))));
            panel.spawn((MenuLabel(usize::MAX-2),Text::new(""),TextFont {font_size:14.,..default()},TextColor(Color::srgb(0.65,0.75,0.8))));
        });
    });
    commands.insert_resource(SceneTarget(target));
    commands.insert_resource(Menu {
        open: false,
        selected: 0,
        settings,
        path,
        supported_msaa,
        difficulty: config.difficulty,
        status,
        maps,
        selected_map,
        destinations,
        travel_open: false,
    });
}
fn msaa(samples: u32) -> Msaa {
    match samples {
        2 => Msaa::Sample2,
        4 => Msaa::Sample4,
        8 => Msaa::Sample8,
        _ => Msaa::Off,
    }
}
fn cycle<T: PartialEq + Copy>(values: &[T], value: T, direction: i32) -> T {
    let index = values.iter().position(|x| *x == value).unwrap_or(0) as i32;
    values[(index + direction).rem_euclid(values.len() as i32) as usize]
}
fn interact(
    config: Res<crate::config::Config>,
    mut physics: ResMut<crate::physics::GamePhysics>,
    mut skater: ResMut<crate::physics::SkaterRuntime>,
    keys: Res<ButtonInput<KeyCode>>,
    mut menu: ResMut<Menu>,
    mut time: ResMut<Time<Virtual>>,
    buttons: Query<(&Interaction, &MenuRow), Changed<Interaction>>,
    travel_buttons: Query<(&Interaction, &TravelRow), Changed<Interaction>>,
    mut exit: MessageWriter<AppExit>,
) {
    if keys.just_pressed(KeyCode::Escape) {
        if menu.open && menu.travel_open {
            menu.travel_open = false;
            menu.selected = 8;
        } else { menu.open = !menu.open; }
    }
    let mut action = None;
    if menu.open {
        if keys.just_pressed(KeyCode::ArrowUp) {
            let count = if menu.travel_open { menu.destinations.len() + 1 } else { 11 };
            menu.selected = (menu.selected + count - 1) % count;
        }
        if keys.just_pressed(KeyCode::ArrowDown) {
            let count = if menu.travel_open { menu.destinations.len() + 1 } else { 11 };
            menu.selected = (menu.selected + 1) % count;
        }
        if !menu.travel_open && keys.just_pressed(KeyCode::ArrowLeft) {
            action = Some((menu.selected, -1));
        }
        if (!menu.travel_open && keys.just_pressed(KeyCode::ArrowRight)) || keys.just_pressed(KeyCode::Enter) {
            action = Some((menu.selected, 1));
        }
        for (interaction, row) in &buttons {
            if !menu.travel_open && *interaction == Interaction::Pressed {
                menu.selected = row.0;
                action = Some((row.0, 1));
            }
        }
    }
    if menu.open && menu.travel_open {
        for (interaction, row) in &travel_buttons {
            if *interaction == Interaction::Pressed {
                menu.selected = row.0;
                action = Some((row.0, 1));
            }
        }
    }
    if let Some((row, direction)) = action {
        if menu.travel_open {
            if let Some(destination) = menu.destinations.get(row) {
                if let Some(transform) = destination.matrix {
                    match skater.travel_to(transform) {
                        Ok(()) => { menu.open = false; menu.travel_open = false; menu.status.clear(); }
                        Err(e) => menu.status = e,
                    }
                }
            } else { menu.travel_open = false; menu.selected = 8; }
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
            2 => {
                menu.settings.samples =
                    cycle(&menu.supported_msaa, menu.settings.samples, direction)
            }
            3 => menu.settings.fps = cycle(LIMITS, menu.settings.fps, direction),
            4 => menu.settings.occlusion = !menu.settings.occlusion,
            5 => {
                menu.difficulty = cycle(&Difficulty::ALL, menu.difficulty, direction);
                physics.set_difficulty(menu.difficulty);
                menu.status = match menu.difficulty.save(&config.asset_root) {
                    Ok(()) => "Difficulty saved".into(),
                    Err(e) => format!("Applied, but could not save: {e}"),
                };
            }
            6 => {
                menu.selected_map = (menu.selected_map as i32 + direction)
                    .rem_euclid(menu.maps.len() as i32) as usize;
                menu.status = "Choose Load map to switch".into();
            }
            7 => {
                match crate::map_library::switch(&config.asset_root, &menu.maps[menu.selected_map]) {
                    Ok(()) => { exit.write(AppExit::Success); }
                    Err(e) => menu.status = e,
                }
            }
            8 => { menu.travel_open = true; menu.selected = 0; menu.status.clear(); },
            9 => menu.open = false,
            10 => {
                exit.write(AppExit::Success);
            }
            _ => {}
        }
        if row < 5 {
            let save = (|| -> Result<(), String> {
                std::fs::create_dir_all(menu.path.parent().unwrap()).map_err(|e| e.to_string())?;
                std::fs::write(
                    &menu.path,
                    serde_json::to_vec_pretty(&menu.settings).map_err(|e| e.to_string())?,
                )
                .map_err(|e| e.to_string())
            })();
            menu.status = match save {
                Ok(()) => "Saved".into(),
                Err(e) => format!("Could not save: {e}"),
            };
        }
        }
    }
    if menu.open {
        time.pause();
    } else {
        time.unpause();
    }
}
fn apply(
    mut commands: Commands,
    menu: Res<Menu>,
    mut window: Single<&mut Window, With<PrimaryWindow>>,
    target: Res<SceneTarget>,
    mut images: ResMut<Assets<Image>>,
    mut cameras: Query<(Entity, &mut Msaa), With<Camera3d>>,
    mut previous: Local<Option<GraphicsSettings>>,
) {
    if previous
        .as_ref()
        .is_none_or(|p| p.width != menu.settings.width || p.height != menu.settings.height)
    {
        window
            .resolution
            .set_physical_resolution(menu.settings.width, menu.settings.height);
    }
    if previous
        .as_ref()
        .is_none_or(|p| p.samples != menu.settings.samples)
    {
        for (_, mut samples) in &mut cameras {
            *samples = msaa(menu.settings.samples);
        }
    }
    if previous.as_ref().is_none_or(|p| p.occlusion != menu.settings.occlusion) {
        for (entity, _) in &cameras {
            if menu.settings.occlusion {
                commands.entity(entity).insert((DepthPrepass, OcclusionCulling));
            } else {
                commands.entity(entity).remove::<(DepthPrepass, OcclusionCulling)>();
            }
        }
        info!("GPU occlusion culling: {}", menu.settings.occlusion);
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
    window: Single<&Window, With<PrimaryWindow>>,
    mut root: Single<&mut Node, With<MenuRoot>>,
    mut labels: Query<(&MenuLabel, &mut Text), Without<StatusLabel>>,
    mut status: Single<&mut Text, With<StatusLabel>>,
    mut buttons: Query<(&MenuRow, &Interaction, &mut BackgroundColor)>,
) {
    root.display = if menu.open {
        Display::Flex
    } else {
        Display::None
    };
    if !menu.open {
        return;
    }
    let s = &menu.settings;
    let size = s.internal_size(window.physical_size());
    for (label, mut text) in &mut labels {
        if label.0 >= usize::MAX-2 {
            let travel = menu.travel_open;
            **text = if label.0 == usize::MAX {
                if travel { "TELEPORT" } else { "PAUSED" }
            } else if label.0 == usize::MAX-1 {
                if travel { "CURRENT MAP LOCATIONS" } else { "GAMEPLAY & GRAPHICS" }
            } else if travel {
                "Click or Enter to travel | Up/Down select\nMouse wheel or drag scrollbar to scroll | Esc back"
            } else { "Click to cycle | Up/Down select | Left/Right change\nEsc resume | Changes save automatically" }.into();
            continue;
        }
        **text = match label.0 {
            0 => format!("Resolution          {} x {}", s.width, s.height),
            1 => format!(
                "Internal resolution   {}%  ({} x {})",
                s.scale, size.x, size.y
            ),
            2 => format!(
                "MSAA                {}",
                if s.samples == 1 {
                    "Off".into()
                } else {
                    format!("{}x", s.samples)
                }
            ),
            3 => format!(
                "FPS limit             {}",
                if s.fps == 0 {
                    "Unlimited".into()
                } else {
                    s.fps.to_string()
                }
            ),
            4 => format!("Occlusion culling     {}", if s.occlusion { "On" } else { "Off" }),
            5 => format!("Difficulty            {}", menu.difficulty.label()),
            6 => format!("Map                   {}", menu.maps[menu.selected_map].label),
            7 => "Load map (restarts session)".into(),
            8 => "Teleport…".into(),
            9 => "Resume".into(),
            _ => "Quit game".into(),
        };
    }
    ***status = menu.status.clone();
    for (row, interaction, mut color) in &mut buttons {
        color.0 = if row.0 == menu.selected || *interaction == Interaction::Hovered {
            Color::srgb(0.10, 0.30, 0.34)
        } else {
            Color::srgb(0.08, 0.11, 0.15)
        };
    }
}
fn travel_scroll(
    menu: Res<Menu>,
    keys: Res<ButtonInput<KeyCode>>,
    mouse: Res<ButtonInput<MouseButton>>,
    mut wheel: MessageReader<bevy::input::mouse::MouseWheel>,
    mut nodes: Query<(&mut Node, Option<&MenuRow>, Option<&TravelPanel>, Option<&TravelThumb>), Without<MenuRoot>>,
    mut viewport: Single<&mut ScrollPosition, With<TravelViewport>>,
    track: Single<&bevy::ui::RelativeCursorPosition, With<TravelTrack>>,
    mut rows: Query<(&TravelRow, &Interaction, &mut BackgroundColor)>,
    mut drag_offset: Local<Option<f32>>,
    mut was_open: Local<bool>,
) {
    let visible = menu.open && menu.travel_open;
    let content = menu.destinations.len() as f32 * TRAVEL_ROW;
    let max_scroll = (content - TRAVEL_HEIGHT).max(0.);
    let thumb_height = (TRAVEL_HEIGHT / content.max(TRAVEL_HEIGHT)) * TRAVEL_HEIGHT;
    if visible && !*was_open { viewport.0.y = 0.; }
    for event in wheel.read() {
        if visible {
            let scale = match event.unit { bevy::input::mouse::MouseScrollUnit::Line => TRAVEL_ROW, _ => 1. };
            viewport.0.y -= event.y * scale;
        }
    }
    if visible && (keys.just_pressed(KeyCode::ArrowUp) || keys.just_pressed(KeyCode::ArrowDown)) && menu.selected < menu.destinations.len() {
        let top = menu.selected as f32 * TRAVEL_ROW;
        viewport.0.y = viewport.0.y.min(top).max(top + TRAVEL_ROW - TRAVEL_HEIGHT);
    }
    if visible && mouse.just_pressed(MouseButton::Left) && track.cursor_over {
        if let Some(cursor) = track.normalized {
            // Bevy uses -0.5..0.5, rather than 0..1, for relative node coordinates.
            let y = (cursor.y + 0.5) * TRAVEL_HEIGHT;
            let top = if max_scroll > 0. { viewport.0.y / max_scroll * (TRAVEL_HEIGHT - thumb_height) } else { 0. };
            *drag_offset = Some(if y >= top && y <= top + thumb_height { y - top } else { thumb_height * 0.5 });
        }
    }
    if !visible || !mouse.pressed(MouseButton::Left) { *drag_offset = None; }
    if let (Some(offset), Some(cursor)) = (*drag_offset, track.normalized) {
        if max_scroll > 0. {
            viewport.0.y = (((cursor.y + 0.5) * TRAVEL_HEIGHT - offset) / (TRAVEL_HEIGHT - thumb_height)) * max_scroll;
        }
    }
    viewport.0.y = viewport.0.y.clamp(0., max_scroll);
    for (mut node, main, panel, thumb) in &mut nodes {
        if main.is_some() { node.display = if menu.travel_open { Display::None } else { Display::Flex }; }
        if panel.is_some() { node.display = if menu.travel_open { Display::Flex } else { Display::None }; }
        if thumb.is_some() {
            node.height = px(thumb_height);
            node.top = px(if max_scroll > 0. { viewport.0.y / max_scroll * (TRAVEL_HEIGHT - thumb_height) } else { 0. });
        }
    }
    for (row, interaction, mut color) in &mut rows {
        color.0 = if row.0 == menu.selected || *interaction == Interaction::Hovered { Color::srgb(0.10,0.30,0.34) } else { Color::srgb(0.08,0.11,0.15) };
    }
    *was_open = visible;
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
        let target = images.add(Image::new_target_texture(1280, 800, TextureFormat::Rgba8UnormSrgb, None));
        app.insert_resource(SceneTarget(target.clone()))
            .insert_resource(images)
            .insert_resource(Menu {
                open: false, selected: 0, settings: GraphicsSettings::default(),
                difficulty: Difficulty::Easy, path: PathBuf::new(), supported_msaa: vec![1, 2, 4, 8], status: String::new(),
                maps: Vec::new(), selected_map: 0, destinations: Vec::new(), travel_open: false,
            })
            .add_systems(Update, apply);
        app.world_mut().spawn((Window::default(), PrimaryWindow));
        let camera = app.world_mut().spawn((Camera3d::default(), Msaa::Off)).id();
        app.update();
        assert!(app.world().entity(camera).contains::<OcclusionCulling>());
        assert!(app.world().entity(camera).contains::<DepthPrepass>());
        {
            let mut menu = app.world_mut().resource_mut::<Menu>();
            menu.settings.occlusion = false;
            menu.settings.samples = 1;
            menu.settings.scale = 67;
        }
        app.update();
        assert!(!app.world().entity(camera).contains::<OcclusionCulling>());
        assert!(!app.world().entity(camera).contains::<DepthPrepass>());
        assert_eq!(*app.world().get::<Msaa>(camera).unwrap(), Msaa::Off);
        assert_eq!(app.world().resource::<Assets<Image>>().get(&target).unwrap().size(), UVec2::new(857, 536));
        {
            let mut menu = app.world_mut().resource_mut::<Menu>();
            menu.settings.occlusion = true;
            menu.settings.samples = 8;
        }
        app.update();
        assert!(app.world().entity(camera).contains::<OcclusionCulling>());
        assert!(app.world().entity(camera).contains::<DepthPrepass>());
        assert_eq!(*app.world().get::<Msaa>(camera).unwrap(), Msaa::Sample8);
    }
    #[test]
    fn travel_scroll_handles_keyboard_drag_and_empty_lists() {
        let mut app = App::new();
        app.insert_resource(Menu {
            open:true, travel_open:true, selected:0, settings:GraphicsSettings::default(),
            path:PathBuf::new(),supported_msaa:vec![1],difficulty:Difficulty::Easy,status:String::new(),maps:vec![],selected_map:0,
            destinations:(0..14).map(|i| crate::teleport_menu::Destination {
                id:i.to_string(),name:i.to_string(),map:"University".into(),
                matrix:Some(skate_core::physics::skeleton_animation_record::IDENTITY),unavailable_reason:None,
            }).collect(),
        }).init_resource::<ButtonInput<KeyCode>>()
            .init_resource::<ButtonInput<MouseButton>>()
            .add_message::<bevy::input::mouse::MouseWheel>()
            .add_systems(Update, travel_scroll);
        let view = app.world_mut().spawn((TravelViewport, ScrollPosition::default())).id();
        app.world_mut().spawn((TravelTrack, bevy::ui::RelativeCursorPosition { cursor_over:true, normalized:Some(Vec2::new(0.,0.5)) }));
        app.world_mut().spawn((TravelThumb, Node::default()));
        app.update();
        app.world_mut().resource_mut::<Menu>().selected = 13;
        app.world_mut().resource_mut::<ButtonInput<KeyCode>>().press(KeyCode::ArrowDown);
        app.update();
        assert_eq!(app.world().get::<ScrollPosition>(view).unwrap().0.y, 294.);
        app.world_mut().resource_mut::<ButtonInput<KeyCode>>().reset_all();
        app.world_mut().get_mut::<ScrollPosition>(view).unwrap().0.y = 0.;
        app.world_mut().resource_mut::<ButtonInput<MouseButton>>().press(MouseButton::Left);
        app.update();
        assert_eq!(app.world().get::<ScrollPosition>(view).unwrap().0.y, 294.);
        app.world_mut().resource_mut::<Menu>().destinations.clear();
        app.update();
        assert_eq!(app.world().get::<ScrollPosition>(view).unwrap().0.y, 0.);
    }
    #[test]
    fn invalid_saved_values_fall_back() {
        let settings: GraphicsSettings =
            serde_json::from_str(r#"{"width":0,"height":999999,"scale":0,"samples":3,"fps":1}"#)
                .unwrap();
        assert_eq!(settings.validated(), GraphicsSettings::default());
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
