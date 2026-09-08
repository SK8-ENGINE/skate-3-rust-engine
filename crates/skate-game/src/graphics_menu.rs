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
    pub(crate) open: bool,
    selected: usize,
    settings: GraphicsSettings,
    path: PathBuf,
    supported_msaa: Vec<u32>,
    difficulty: Difficulty,
    status: String,
    maps: Vec<crate::map_library::Entry>,
    selected_map: usize,
}
impl Menu {
    pub(crate) fn transition_finished(&mut self, status: String, resume: bool) {
        self.status = status;
        self.open = !resume;
    }
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
pub(crate) struct MenuRow(usize);
#[derive(Component)]
struct MenuLabel(usize);
#[derive(Component)]
struct StatusLabel;

#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub(crate) struct MenuInput;

pub(crate) struct GraphicsMenuPlugin;
impl Plugin for GraphicsMenuPlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(FramePacer(Instant::now()))
            .add_systems(PostStartup, setup)
            .add_systems(PreUpdate, interact.in_set(MenuInput).after(bevy::input::InputSystems))
            .add_systems(Update, (apply, labels).chain())
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
    commands.spawn((MenuRoot, UiTargetCamera(output), GlobalZIndex(10), Node {
        display: Display::None, width:percent(100), height:percent(100), align_items:AlignItems::Center,
        justify_content:JustifyContent::Center, position_type:PositionType::Absolute, ..default()
    }, BackgroundColor(Color::srgba(0.015,0.025,0.04,0.88)))).with_children(|root| {
        root.spawn((Node { width:px(560),max_width:percent(95),padding:UiRect::all(px(18)),flex_direction:FlexDirection::Column,row_gap:px(6),border_radius:BorderRadius::all(px(12)),..default() },
            BackgroundColor(Color::srgb(0.035,0.055,0.08)))).with_children(|panel| {
            panel.spawn((Text::new("PAUSED"),TextFont {font_size:32.,..default()},TextColor(Color::WHITE)));
            panel.spawn((Text::new("GAMEPLAY & GRAPHICS"),TextFont {font_size:16.,..default()},TextColor(Color::srgb(0.4,0.85,0.85))));
            for i in 0..12 {
                panel.spawn((Button, MenuRow(i), Node {width:percent(100),min_height:px(36),padding:UiRect::all(px(8)),align_items:AlignItems::Center,border_radius:BorderRadius::all(px(5)),..default()},
                    BackgroundColor(Color::srgb(0.08,0.11,0.15)))).with_children(|row| {
                    row.spawn((MenuLabel(i),Text::new(""),TextFont {font_size:18.,..default()},TextColor(Color::WHITE)));
                });
            }
            panel.spawn((StatusLabel,Text::new(""),TextFont {font_size:15.,..default()},TextColor(Color::srgb(0.65,0.75,0.8))));
            panel.spawn((Text::new("Click to cycle | Up/Down select | Left/Right change\nEsc resume | Changes save automatically"),TextFont {font_size:14.,..default()},TextColor(Color::srgb(0.65,0.75,0.8))));
        });
    });
    commands.insert_resource(SceneTarget(target));
    let maps = crate::map_library::discover(&config.asset_root);
    let selected_map = maps.iter().position(|m| m.path.as_ref() == config.map_path.as_ref()).unwrap_or(0);
    if config.start_paused { time.pause(); }
    commands.insert_resource(Menu {
        open: config.start_paused,
        selected: 0,
        settings,
        path,
        supported_msaa,
        difficulty: config.difficulty,
        status: String::new(),
        maps,
        selected_map,
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
pub(crate) fn interact(
    mut config: ResMut<crate::config::Config>,
    mut transition: ResMut<crate::map_transition::MapTransition>,
    mut customiser: ResMut<crate::customiser::Customiser>,
    mut mods: ResMut<crate::modding::ModMenu>,
    nav: Res<crate::customiser::Navigation>,
    mut physics: ResMut<crate::physics::GamePhysics>,
    keys: Res<ButtonInput<KeyCode>>,
    mut menu: ResMut<Menu>,
    mut time: ResMut<Time<Virtual>>,
    buttons: Query<(&Interaction, &MenuRow), Changed<Interaction>>,
    mut exit: MessageWriter<AppExit>,
) {
    if transition.busy() {
        menu.open = true;
        time.pause();
        return;
    }
    if customiser.open || mods.open { return; }
    if keys.just_pressed(KeyCode::Escape) || nav.pressed & 0x10 != 0 {
        menu.open = !menu.open;
    }
    let mut action = None;
    if menu.open {
        if keys.just_pressed(KeyCode::ArrowUp) || nav.pressed & 1 != 0 {
            menu.selected = (menu.selected + 11) % 12;
        }
        if keys.just_pressed(KeyCode::ArrowDown) || nav.pressed & 2 != 0 {
            menu.selected = (menu.selected + 1) % 12;
        }
        if keys.just_pressed(KeyCode::ArrowLeft) || nav.pressed & 4 != 0 {
            action = Some((menu.selected, -1));
        }
        if keys.just_pressed(KeyCode::ArrowRight) || keys.just_pressed(KeyCode::Enter) || nav.pressed & (8 | 0x1000) != 0 {
            action = Some((menu.selected, 1));
        }
        for (interaction, row) in &buttons {
            if *interaction == Interaction::Pressed {
                menu.selected = row.0;
                action = Some((row.0, 1));
            }
        }
    }
    if let Some((row, direction)) = action {
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
                config.difficulty = menu.difficulty;
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
                transition.request(menu.maps[menu.selected_map].clone());
                menu.status = "Loading map...".into();
            }
            8 => menu.open = false,
            9 => {
                exit.write(AppExit::Success);
            }
            10 => customiser.begin(),
            11 => mods.begin(),
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
    transition: Res<crate::map_transition::MapTransition>,
    time: Res<Time<Real>>,
    customiser: Res<crate::customiser::Customiser>,
    mods: Res<crate::modding::ModMenu>,
    window: Single<&Window, With<PrimaryWindow>>,
    mut root: Single<&mut Node, With<MenuRoot>>,
    mut labels: Query<(&MenuLabel, &mut Text), Without<StatusLabel>>,
    mut status: Single<&mut Text, With<StatusLabel>>,
    mut buttons: Query<(&MenuRow, &Interaction, &mut BackgroundColor)>,
) {
    root.display = if menu.open && !customiser.open && !mods.open {
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
            7 => if transition.busy() { "Loading map...".into() } else { "Load map".into() },
            8 => "Resume".into(),
            9 => "Quit game".into(),
            10 => "Character customiser".into(),
            _ => "Mods".into(),
        };
    }
    ***status = if transition.busy() {
        format!("{} {}\nGameplay is paused. Please wait.", ["|", "/", "-", "\\"][(time.elapsed_secs() * 4.) as usize % 4], transition.label())
    } else { menu.status.clone() };
    for (row, interaction, mut color) in &mut buttons {
        color.0 = if row.0 == menu.selected || *interaction == Interaction::Hovered {
            Color::srgb(0.10, 0.30, 0.34)
        } else {
            Color::srgb(0.08, 0.11, 0.15)
        };
    }
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
                maps: vec![crate::map_library::Entry { label: "Test world".into(), path: None }], selected_map: 0,
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
