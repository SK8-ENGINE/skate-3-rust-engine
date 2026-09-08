//! Enabled mod settings beside the normal pause menu, with independent focus.
use super::{ModMenu, Mods};
use bevy::prelude::*;
#[derive(Resource, Default)]
pub(crate) struct EnabledPanel {
    pub focused: bool,
    selected: usize,
    status: String,
}
#[derive(Component)]
struct Root;
#[derive(Component)]
struct Row(usize);
#[derive(Component)]
struct Label(usize);
#[derive(Component)]
struct Hint;
#[derive(Clone)]
enum Entry {
    Mod(String),
    Setting(String, String),
}
fn entries(mods: &Mods) -> Vec<(String, Entry)> {
    let mut rows = vec![];
    for (id, p) in &mods.manager.packages {
        if p.running() {
            rows.push((
                format!("{}  -  ENABLED", p.manifest.name),
                Entry::Mod(id.clone()),
            ));
            for (key, s) in &p.manifest.settings {
                let value = &p.settings[key];
                let display = value
                    .as_f64()
                    .map(|v| format!("{v:.2}"))
                    .unwrap_or_else(|| {
                        value
                            .as_str()
                            .map(str::to_owned)
                            .unwrap_or_else(|| value.to_string())
                    });
                rows.push((
                    format!("{}   {}", s.label, display),
                    Entry::Setting(id.clone(), key.clone()),
                ));
            }
        }
    }
    rows
}
pub(super) fn install(app: &mut App) {
    app.init_resource::<EnabledPanel>()
        .add_systems(PostStartup, setup)
        .add_systems(
            PreUpdate,
            input
                .after(crate::graphics_menu::MenuInput)
                .before(crate::map_transition::MapTransitionSet),
        )
        .add_systems(Update, draw);
}
fn setup(mut commands: Commands) {
    commands
        .spawn((
            Root,
            GlobalZIndex(12),
            Node {
                display: Display::None,
                position_type: PositionType::Absolute,
                left: percent(3),
                top: percent(8),
                width: percent(39),
                padding: UiRect::all(px(16)),
                row_gap: px(7),
                flex_direction: FlexDirection::Column,
                border_radius: BorderRadius::all(px(12)),
                ..default()
            },
            BackgroundColor(Color::srgb(0.035, 0.055, 0.08)),
        ))
        .with_children(|panel| {
            panel.spawn((
                Text::new("ENABLED MOD SETTINGS"),
                TextFont {
                    font_size: 23.,
                    ..default()
                },
                TextColor(Color::srgb(0.25, 1., 0.4)),
            ));
            for i in 0..10 {
                panel
                    .spawn((
                        Row(i),
                        Button,
                        Node {
                            padding: UiRect::all(px(7)),
                            min_height: px(32),
                            ..default()
                        },
                        BackgroundColor(Color::srgb(0.08, 0.11, 0.15)),
                    ))
                    .with_children(|row| {
                        row.spawn((
                            Label(i),
                            Text::new(""),
                            TextFont {
                                font_size: 16.,
                                ..default()
                            },
                            TextColor(Color::WHITE),
                        ));
                    });
            }
            panel.spawn((
                Hint,
                Text::new(""),
                TextFont {
                    font_size: 14.,
                    ..default()
                },
                TextColor(Color::srgb(0.65, 0.85, 0.85)),
            ));
        });
}
fn visible(
    pause: &crate::graphics_menu::Menu,
    menu: &ModMenu,
    custom: &crate::customiser::Customiser,
) -> bool {
    pause.open && !menu.open && !custom.open
}
fn input(
    mut panel: ResMut<EnabledPanel>,
    pause: Res<crate::graphics_menu::Menu>,
    mut menu: ResMut<ModMenu>,
    custom: Res<crate::customiser::Customiser>,
    mut mods: ResMut<Mods>,
    keys: Res<ButtonInput<KeyCode>>,
    nav: Res<crate::customiser::Navigation>,
    buttons: Query<(&Interaction, &Row), Changed<Interaction>>,
) {
    let rows = entries(&mods);
    if !visible(&pause, &menu, &custom) || rows.is_empty() {
        panel.focused = false;
        return;
    }
    if keys.just_pressed(KeyCode::Tab) || nav.pressed & 0x4000 != 0 {
        panel.focused = !panel.focused;
    }
    panel.selected = panel.selected.min(rows.len() - 1);
    let mut direction = 0;
    if panel.focused {
        if keys.just_pressed(KeyCode::ArrowUp) || nav.pressed & 1 != 0 {
            panel.selected = (panel.selected + rows.len() - 1) % rows.len();
        }
        if keys.just_pressed(KeyCode::ArrowDown) || nav.pressed & 2 != 0 {
            panel.selected = (panel.selected + 1) % rows.len();
        }
        if keys.just_pressed(KeyCode::ArrowLeft) || nav.pressed & 4 != 0 {
            direction = -1;
        }
        if keys.just_pressed(KeyCode::ArrowRight)
            || keys.just_pressed(KeyCode::Enter)
            || nav.pressed & (8 | 0x1000) != 0
        {
            direction = 1;
        }
    }
    let offset = panel.selected / 10 * 10;
    for (interaction, row) in &buttons {
        if *interaction == Interaction::Pressed && offset + row.0 < rows.len() {
            panel.focused = true;
            panel.selected = offset + row.0;
            direction = 1;
        }
    }
    if direction == 0 {
        return;
    }
    match &rows[panel.selected].1 {
        Entry::Mod(id) => menu.configure(id.clone()),
        Entry::Setting(id, key) => {
            let p = &mods.manager.packages[id];
            let s = &p.manifest.settings[key];
            let v = &p.settings[key];
            let next = match s.kind.as_str() {
                "number" => Some(serde_json::json!(
                    (v.as_f64().unwrap() + direction as f64 * s.step.unwrap())
                        .clamp(s.min.unwrap(), s.max.unwrap())
                )),
                "boolean" => Some(serde_json::json!(!v.as_bool().unwrap())),
                "choice" => {
                    let i = s
                        .choices
                        .iter()
                        .position(|s| Some(s.as_str()) == v.as_str())
                        .unwrap_or(0);
                    Some(serde_json::json!(
                        s.choices
                            [(i as i32 + direction).rem_euclid(s.choices.len() as i32) as usize]
                    ))
                }
                _ => {
                    menu.configure(id.clone());
                    None
                }
            };
            if let Some(v) = next {
                panel.status = mods.manager.setting(id, key, v).err().unwrap_or_default();
            }
        }
    }
}
fn draw(
    panel: Res<EnabledPanel>,
    pause: Res<crate::graphics_menu::Menu>,
    menu: Res<ModMenu>,
    custom: Res<crate::customiser::Customiser>,
    mods: Res<Mods>,
    mut root: Single<&mut Node, With<Root>>,
    mut rows: Query<(&Row, &mut Node, &mut BackgroundColor), Without<Root>>,
    mut labels: Query<(&Label, &mut Text), Without<Hint>>,
    mut hint: Single<&mut Text, (With<Hint>, Without<Label>)>,
) {
    let list = entries(&mods);
    root.display = if visible(&pause, &menu, &custom) && !list.is_empty() {
        Display::Flex
    } else {
        Display::None
    };
    let offset = panel.selected / 10 * 10;
    for (label, mut text) in &mut labels {
        **text = list
            .get(offset + label.0)
            .map(|e| e.0.clone())
            .unwrap_or_default();
    }
    for (row, mut node, mut color) in &mut rows {
        node.display = if offset + row.0 < list.len() {
            Display::Flex
        } else {
            Display::None
        };
        color.0 = if panel.focused && offset + row.0 == panel.selected {
            Color::srgb(0.10, 0.30, 0.34)
        } else {
            Color::srgb(0.08, 0.11, 0.15)
        };
    }
    let description = list
        .get(panel.selected)
        .and_then(|(_, entry)| match entry {
            Entry::Setting(id, key) => Some(
                mods.manager.packages[id].manifest.settings[key]
                    .description
                    .as_str(),
            ),
            _ => None,
        })
        .unwrap_or("Select a mod name for its enable/disable controls.");
    ***hint = format!(
        "Tab / X: {}\nUp/Down select | Left/Right adjust | Click increases\n{}\n{}",
        if panel.focused {
            "return to pause controls"
        } else {
            "focus mod settings"
        },
        description,
        panel.status
    );
}
