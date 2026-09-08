//! Draft/commit character editing over the existing retail GLB and animation rig.
use bevy::{asset::LoadState, gltf::Gltf, prelude::*};
use serde::Deserialize;
use serde_json::{Value, json};
use std::{
    io::Write,
    path::PathBuf,
    process::{Command, Stdio},
    sync::{Mutex, mpsc},
};

#[derive(Clone, Default, Deserialize)]
struct Entry {
    label: String,
    #[serde(default)]
    children: Vec<Entry>,
    patch: Option<Value>,
    scalar: Option<String>,
    note: Option<String>,
    minimum: Option<f64>,
    maximum: Option<f64>,
    initial: Option<f64>,
    step: Option<f64>,
}
#[derive(Clone, Deserialize)]
struct Worker {
    python: PathBuf,
    working_directory: PathBuf,
}
#[derive(Resource, Default)]
pub(crate) struct Navigation {
    pub pressed: u16,
    previous: u16,
}
#[derive(Resource)]
pub(crate) struct Customiser {
    pub open: bool,
    just_opened: bool,
    index: Entry,
    path: Vec<usize>,
    selected: usize,
    draft: Value,
    committed: Value,
    scene: String,
    committed_scene: String,
    config: PathBuf,
    settings: PathBuf,
    worker: Option<Worker>,
    receiver: Option<Mutex<mpsc::Receiver<Result<Value, String>>>>,
    loading: Option<(Handle<Gltf>, String, Value)>,
    status: String,
    redraw: bool,
}
impl Customiser {
    pub(crate) fn begin(&mut self) {
        self.open = true;
        self.just_opened = true;
        self.path.clear();
        self.selected = 0;
        self.status = "Choose a category. Changes are previews until you Save.".into();
        self.redraw = true;
    }
    fn page(&self) -> &Entry {
        let mut page = &self.index;
        for &i in &self.path {
            page = &page.children[i];
        }
        page
    }
    fn busy(&self) -> bool {
        self.receiver.is_some() || self.loading.is_some()
    }
    fn request(&mut self, profile: Value) {
        if self.busy() {
            self.status = "Please wait for the current preview.".into();
            return;
        }
        let Some(worker) = self.worker.clone() else {
            self.status = "Private customisation worker is not configured.".into();
            return;
        };
        let config = self.config.clone();
        let (tx, rx) = mpsc::channel();
        self.receiver = Some(Mutex::new(rx));
        self.status = "Preparing retail meshes and textures…".into();
        self.redraw = true;
        std::thread::spawn(move || {
            let run = || -> Result<Value, String> {
                let mut command = Command::new(&worker.python);
                command
                    .current_dir(&worker.working_directory)
                    .args([
                        "-m",
                        "tools.asset_pipeline.customisation_worker",
                        "--config",
                    ])
                    .arg(config)
                    .stdin(Stdio::piped())
                    .stdout(Stdio::piped())
                    .stderr(Stdio::piped());
                #[cfg(windows)]
                {
                    use std::os::windows::process::CommandExt;
                    command.creation_flags(0x08000000);
                }
                let mut child = command
                    .spawn()
                    .map_err(|e| format!("Cannot start asset worker: {e}"))?;
                child
                    .stdin
                    .take()
                    .ok_or("Worker stdin missing")?
                    .write_all(&serde_json::to_vec(&profile).map_err(|e| e.to_string())?)
                    .map_err(|e| e.to_string())?;
                let output = child.wait_with_output().map_err(|e| e.to_string())?;
                let response: Value = serde_json::from_slice(&output.stdout).map_err(|e| {
                    format!(
                        "Worker response: {e}; {}",
                        String::from_utf8_lossy(&output.stderr)
                            .chars()
                            .take(400)
                            .collect::<String>()
                    )
                })?;
                if let Some(e) = response.get("error").and_then(Value::as_str) {
                    return Err(e.into());
                }
                if !output.status.success() {
                    return Err("Asset worker failed".into());
                }
                Ok(response)
            };
            let _ = tx.send(run());
        });
    }
}
#[derive(Component)]
struct Root;
#[derive(Component)]
struct Row(usize);
pub(crate) struct CustomiserPlugin;
impl Plugin for CustomiserPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Navigation>()
            .add_systems(PreUpdate, navigation)
            .add_systems(PostStartup, setup)
            .add_systems(
                Update,
                (interact, complete, draw)
                    .chain()
                    .after(crate::graphics_menu::interact)
                    .before(crate::app::FrameSet::Animation),
            );
    }
}
fn navigation(mut nav: ResMut<Navigation>) {
    let current = (0..4)
        .find_map(|i| crate::input::platform::poll(i).ok())
        .map_or(0, |p| p.state.buttons);
    nav.pressed = current & !nav.previous;
    nav.previous = current;
}
fn setup(
    mut commands: Commands,
    config: Res<crate::config::Config>,
    manifest: Res<crate::assets::AssetManifest>,
) {
    let root = config.asset_root.join("private/customisation");
    let read = |name: &str| {
        std::fs::read(root.join(name))
            .ok()
            .and_then(|b| serde_json::from_slice::<Value>(&b).ok())
    };
    let index = read("menu.json")
        .and_then(|v| serde_json::from_value(v).ok())
        .unwrap_or(Entry {
            label: "Character customiser".into(),
            note: Some("Prepare owned customisation assets for this installation.".into()),
            ..default()
        });
    let defaults = read("default.json")
        .unwrap_or(json!({"selections":{},"morphs":{},"truck":0.7,"wheel":0.7,"posture":0}));
    let settings = config
        .asset_root
        .parent()
        .unwrap_or(&config.asset_root)
        .join("settings/character.json");
    let saved = std::fs::read(&settings)
        .ok()
        .and_then(|b| serde_json::from_slice::<Value>(&b).ok());
    let worker = read("worker.json").and_then(|v| serde_json::from_value(v).ok());
    let mut state = Customiser {
        open: false,
        just_opened: false,
        index,
        path: vec![],
        selected: 0,
        draft: defaults.clone(),
        committed: defaults,
        scene: manifest.0.character_scene.clone(),
        committed_scene: manifest.0.character_scene.clone(),
        config: root.join("worker.json"),
        settings,
        worker,
        receiver: None,
        loading: None,
        status: String::new(),
        redraw: true,
    };
    if let Some(profile) = saved {
        state.committed = profile.clone();
        state.request(profile);
    }
    commands.insert_resource(state);
    commands.spawn((
        Root,
        GlobalZIndex(20),
        Node {
            display: Display::None,
            position_type: PositionType::Absolute,
            width: percent(42),
            height: percent(100),
            padding: UiRect::all(px(20)),
            flex_direction: FlexDirection::Column,
            row_gap: px(6),
            ..default()
        },
        BackgroundColor(Color::srgba(0.025, 0.04, 0.06, 0.98)),
    ));
}
fn merge(target: &mut Value, patch: &Value) {
    if let (Some(a), Some(b)) = (target.as_object_mut(), patch.as_object()) {
        for (k, v) in b {
            merge(a.entry(k.clone()).or_insert(Value::Null), v);
        }
    } else {
        *target = patch.clone();
    }
}
fn scalar(profile: &Value, key: &str, initial: f64) -> f64 {
    let pointer = format!("/{}", key.replace('.', "/"));
    profile
        .pointer(&pointer)
        .and_then(Value::as_f64)
        .unwrap_or(initial)
}
fn interact(
    keys: Res<ButtonInput<KeyCode>>,
    nav: Res<Navigation>,
    mut state: ResMut<Customiser>,
    mut menu: ResMut<crate::graphics_menu::Menu>,
    server: Res<AssetServer>,
    buttons: Query<(&Interaction, &Row), Changed<Interaction>>,
) {
    if !state.open {
        return;
    }
    if state.just_opened {
        state.just_opened = false;
        return;
    }
    let mut action = None;
    let count = state.page().children.len();
    if count > 0 {
        if keys.just_pressed(KeyCode::ArrowUp) || nav.pressed & 1 != 0 {
            state.selected = (state.selected + count - 1) % count;
            state.redraw = true;
        }
        if keys.just_pressed(KeyCode::ArrowDown) || nav.pressed & 2 != 0 {
            state.selected = (state.selected + 1) % count;
            state.redraw = true;
        }
        if keys.just_pressed(KeyCode::Enter)
            || keys.just_pressed(KeyCode::ArrowRight)
            || nav.pressed & (0x1000 | 8) != 0
        {
            action = Some((state.selected, 1));
        }
        if keys.just_pressed(KeyCode::ArrowLeft) || nav.pressed & 4 != 0 {
            action = Some((state.selected, -1));
        }
    }
    let mut save = keys.just_pressed(KeyCode::F5) || nav.pressed & 0x4000 != 0;
    let mut back = keys.just_pressed(KeyCode::Escape)
        || keys.just_pressed(KeyCode::Backspace)
        || nav.pressed & 0x2000 != 0;
    let mut reset = false;
    for (interaction, row) in &buttons {
        if *interaction == Interaction::Pressed {
            match row.0 {
                usize::MAX => save = true,
                x if x == usize::MAX - 1 => back = true,
                x if x == usize::MAX - 2 => reset = true,
                i => {
                    state.selected = i;
                    action = Some((i, 1));
                }
            }
        }
    }
    if save {
        if state.busy() {
            state.status = "Wait for the preview before saving.".into();
        } else {
            let result = (|| -> Result<(), String> {
                std::fs::create_dir_all(state.settings.parent().unwrap())
                    .map_err(|e| e.to_string())?;
                let temp = state.settings.with_extension("pending.json");
                std::fs::write(
                    &temp,
                    serde_json::to_vec_pretty(&state.draft).map_err(|e| e.to_string())?,
                )
                .map_err(|e| e.to_string())?;
                // Same directory replacement preserves the previous committed profile on write failure.
                std::fs::rename(&temp, &state.settings).map_err(|e| e.to_string())
            })();
            match result {
                Ok(()) => {
                    state.committed = state.draft.clone();
                    state.committed_scene = state.scene.clone();
                    state.status = "Saved. Back returns to skating with this character.".into();
                }
                Err(e) => state.status = format!("Could not save: {e}"),
            }
        }
        state.redraw = true;
    }
    if reset && !state.busy() {
        let profile = std::fs::read(state.config.with_file_name("default.json"))
            .ok()
            .and_then(|b| serde_json::from_slice(&b).ok());
        if let Some(profile) = profile {
            state.request(profile);
        }
    }
    if back {
        if !state.path.is_empty() {
            state.selected = state.path.pop().unwrap();
        } else if state.busy() {
            state.status = "Wait for the preview before leaving.".into();
        } else {
            if state.draft != state.committed {
                let scene = state.committed_scene.clone();
                let profile = state.committed.clone();
                state.loading = Some((server.load(scene.clone()), scene, profile));
            }
            state.open = false;
            menu.open = false;
        }
        state.redraw = true;
        return;
    }
    if let Some((i, direction)) = action {
        let Some(entry) = state.page().children.get(i).cloned() else {
            return;
        };
        if !entry.children.is_empty() {
            state.path.push(i);
            state.selected = 0;
        } else if let Some(note) = entry.note {
            state.status = note;
        } else if !state.busy() {
            let mut profile = state.draft.clone();
            if let Some(key) = entry.scalar {
                let current = scalar(&profile, &key, entry.initial.unwrap_or(0.));
                let value = (current + direction as f64 * entry.step.unwrap_or(0.1))
                    .clamp(entry.minimum.unwrap_or(0.), entry.maximum.unwrap_or(1.));
                if let Some((parent, child)) = key.split_once('.') {
                    profile[parent][child] = json!(value);
                } else {
                    profile[&key] = json!(value);
                }
                // Equipment controls are consumed directly by existing native physics.
                if key == "truck" || key == "wheel" {
                    state.draft = profile;
                } else {
                    state.request(profile);
                }
            } else if let Some(patch) = entry.patch {
                merge(&mut profile, &patch);
                if patch.get("posture").is_some() {
                    state.draft = profile;
                    state.status="Posture is selected. The native motion tree applies it when skating resumes.".into();
                } else {
                    state.request(profile);
                }
            }
        } else {
            state.status = "Please wait for the current preview.".into();
        }
        state.redraw = true;
    }
}
fn complete(
    mut state: ResMut<Customiser>,
    server: Res<AssetServer>,
    mut commands: Commands,
    root: Query<Entity, With<crate::world::PlayerRoot>>,
    scenes: Query<(Entity, &ChildOf), With<SceneRoot>>,
    mut animation: ResMut<crate::animation::AnimationStatus>,
    mut physics: ResMut<crate::physics::GamePhysics>,
    mut skater: ResMut<crate::physics::SkaterRuntime>,
) {
    let response = state
        .receiver
        .as_ref()
        .and_then(|rx| rx.lock().ok()?.try_recv().ok());
    if let Some(response) = response {
        state.receiver = None;
        state.redraw = true;
        match response {
            Ok(value) => {
                if let (Some(scene), Some(profile)) = (
                    value.get("scene").and_then(Value::as_str),
                    value.get("profile"),
                ) {
                    state.loading =
                        Some((server.load(scene.to_owned()), scene.into(), profile.clone()));
                    state.status = "Loading the assembled character…".into();
                } else {
                    state.status = "Asset worker returned an incomplete character.".into();
                }
            }
            Err(e) => state.status = format!("Preview unchanged: {e}"),
        }
    }
    if let Some((handle, _, _)) = &state.loading {
        if let Some(LoadState::Failed(error)) = server.get_load_state(handle.id()) {
            state.status = format!("Preview unchanged: {error}");
            state.loading = None;
            state.redraw = true;
        } else if server.is_loaded_with_dependencies(handle.id()) {
            if let Ok(root) = root.single() {
                let (_, scene, profile) = state.loading.take().unwrap();
                for (entity, parent) in &scenes {
                    if parent.parent() == root {
                        commands.entity(entity).despawn();
                    }
                }
                commands.entity(root).with_children(|p| {
                    p.spawn(SceneRoot(
                        server.load(GltfAssetLabel::Scene(0).from_asset(scene.clone())),
                    ));
                });
                *animation = default();
                if profile == state.committed {
                    state.committed_scene = scene.clone();
                }
                state.scene = scene;
                state.draft = profile;
                state.status = "Preview ready. Save to keep, or Back at the top to discard.".into();
                state.redraw = true;
            }
        }
    }
    physics.set_equipment_preferences(
        scalar(&state.draft, "truck", 0.7).clamp(0., 1.) as f32,
        scalar(&state.draft, "wheel", 0.7).clamp(0., 1.) as f32,
    );
    skater
        .animation
        .motion
        .animation
        .posture
        .set_profile(scalar(&state.draft, "posture", 0.).clamp(0., 3.) as u32);
}
fn draw(
    mut commands: Commands,
    mut state: ResMut<Customiser>,
    mut root: Single<(Entity, &mut Node), With<Root>>,
) {
    root.1.display = if state.open {
        Display::Flex
    } else {
        Display::None
    };
    if !state.open || !state.redraw {
        return;
    }
    state.redraw = false;
    commands.entity(root.0).despawn_children();
    let page = state.page();
    let start = state.selected / 10 * 10;
    commands.entity(root.0).with_children(|p| {
        p.spawn((
            Text::new(&page.label),
            TextFont {
                font_size: 25.,
                ..default()
            },
            TextColor(Color::WHITE),
        ));
        p.spawn((
            Text::new("RETAIL CHARACTER • TEST BUILD"),
            TextFont {
                font_size: 13.,
                ..default()
            },
            TextColor(Color::srgb(0.4, 0.85, 0.8)),
        ));
        for (i, entry) in page.children.iter().enumerate().skip(start).take(10) {
            let value = entry
                .scalar
                .as_ref()
                .map(|key| {
                    format!(
                        "  {:.3}",
                        scalar(&state.draft, key, entry.initial.unwrap_or(0.))
                    )
                })
                .unwrap_or_default();
            let suffix = if !entry.children.is_empty() {
                "  >"
            } else {
                ""
            };
            p.spawn((
                Button,
                Row(i),
                Node {
                    width: percent(100),
                    min_height: px(32),
                    padding: UiRect::all(px(7)),
                    ..default()
                },
                BackgroundColor(if state.selected == i {
                    Color::srgb(0.08, 0.32, 0.33)
                } else {
                    Color::srgb(0.06, 0.10, 0.14)
                }),
            ))
            .with_children(|r| {
                r.spawn((
                    Text::new(format!("{}{value}{suffix}", entry.label)),
                    TextFont {
                        font_size: 16.,
                        ..default()
                    },
                    TextColor(Color::WHITE),
                ));
            });
        }
        if page.children.len() > 10 {
            p.spawn((
                Text::new(format!(
                    "{}–{} / {} • Up/Down to browse",
                    start + 1,
                    (start + 10).min(page.children.len()),
                    page.children.len()
                )),
                TextFont {
                    font_size: 13.,
                    ..default()
                },
            ));
        }
        for (id, title) in [
            (usize::MAX, "Save  [F5 / X]"),
            (usize::MAX - 1, "Back  [Esc / B]"),
            (usize::MAX - 2, "Restore original character"),
        ] {
            p.spawn((
                Button,
                Row(id),
                Node {
                    padding: UiRect::all(px(7)),
                    ..default()
                },
                BackgroundColor(Color::srgb(0.10, 0.17, 0.21)),
            ))
            .with_children(|r| {
                r.spawn((
                    Text::new(title),
                    TextFont {
                        font_size: 16.,
                        ..default()
                    },
                    TextColor(Color::WHITE),
                ));
            });
        }
        p.spawn((
            Text::new(&state.status),
            TextFont {
                font_size: 14.,
                ..default()
            },
            TextColor(Color::srgb(0.8, 0.85, 0.9)),
        ));
        p.spawn((
            Text::new(
                "Enter / A: choose • Left/Right: adjust\nBack at the top discards unsaved changes.",
            ),
            TextFont {
                font_size: 13.,
                ..default()
            },
            TextColor(Color::srgb(0.55, 0.65, 0.7)),
        ));
    });
}
