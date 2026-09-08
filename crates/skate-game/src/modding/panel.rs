//! Independent, draggable enabled-mod windows. Positions survive closing Escape.
use super::{ModMenu, Mods};
use bevy::prelude::*;
use std::collections::BTreeMap;
const PAGE: usize = 7;
#[derive(Default)]
struct Layout {
    position: Vec2,
    collapsed: bool,
    selected: usize,
    status: String,
}
#[derive(Resource, Default)]
pub(crate) struct EnabledPanel {
    pub focused: bool,
    active: Option<String>,
    layouts: BTreeMap<String, Layout>,
    drag: Option<(String, Vec2)>,
    signature: Vec<(String, Vec<String>)>,
}
impl EnabledPanel {
    pub fn dragging(&self) -> bool {
        self.drag.is_some()
    }
}
#[derive(Component)]
struct Root(String);
#[derive(Component)]
struct Body(String);
#[derive(Component)]
struct Header(String);
#[derive(Component)]
struct Row(String, usize);
#[derive(Component)]
struct Label(String, usize);
#[derive(Component)]
struct ValueLabel(String, usize);
#[derive(Component)]
struct Hint(String);
#[derive(Component)]
struct CollapseLabel(String);
#[derive(Component, Clone)]
struct Action(String, Operation);
#[derive(Clone)]
enum Operation {
    Adjust(usize, i32),
    Collapse,
    Configure,
    Page(i32),
}
pub(super) fn install(app: &mut App) {
    app.init_resource::<EnabledPanel>()
        .add_systems(
            PreUpdate,
            input
                .after(crate::graphics_menu::MenuInput)
                .before(crate::map_transition::MapTransitionSet),
        )
        .add_systems(Update, (sync, draw).chain());
}
fn button(parent: &mut ChildSpawnerCommands, text: &str, action: Action) {
    parent
        .spawn((
            Button,
            action,
            Node {
                min_width: px(28.),
                height: px(28.),
                padding: UiRect::horizontal(px(5.)),
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
                ..default()
            },
            BackgroundColor(Color::srgb(0.12, 0.20, 0.26)),
        ))
        .with_children(|p| {
            p.spawn((
                Text::new(text),
                TextFont {
                    font_size: 16.,
                    ..default()
                },
                TextColor(Color::WHITE),
            ));
        });
}
fn sync(
    mut commands: Commands,
    mods: Res<Mods>,
    mut panel: ResMut<EnabledPanel>,
    roots: Query<Entity, With<Root>>,
) {
    let signature: Vec<_> = mods
        .manager
        .packages
        .iter()
        .filter(|(_, p)| p.running())
        .map(|(id, p)| {
            (
                id.clone(),
                p.manifest.settings.keys().cloned().collect::<Vec<_>>(),
            )
        })
        .collect();
    if signature == panel.signature {
        return;
    }
    for root in &roots {
        commands.entity(root).despawn();
    }
    panel.signature = signature.clone();
    panel.drag = None;
    if !signature
        .iter()
        .any(|(id, _)| panel.active.as_ref() == Some(id))
    {
        panel.active = signature.first().map(|(id, _)| id.clone());
        panel.focused = false;
    }
    for (index, (id, _)) in signature.iter().enumerate() {
        panel.layouts.entry(id.clone()).or_insert_with(|| Layout {
            position: Vec2::new(16. + index as f32 * 22., 64. + index as f32 * 34.),
            ..default()
        });
        let name = &mods.manager.packages[id].manifest.name;
        commands
            .spawn((
                Root(id.clone()),
                GlobalZIndex(12),
                Node {
                    display: Display::None,
                    position_type: PositionType::Absolute,
                    width: px(320.),
                    max_width: percent(95),
                    padding: UiRect::all(px(10.)),
                    row_gap: px(8.),
                    flex_direction: FlexDirection::Column,
                    border_radius: BorderRadius::all(px(9.)),
                    ..default()
                },
                BackgroundColor(Color::srgb(0.035, 0.055, 0.08)),
            ))
            .with_children(|window| {
                window
                    .spawn(Node {
                        width: percent(100),
                        column_gap: px(5.),
                        ..default()
                    })
                    .with_children(|bar| {
                        bar.spawn((
                            Button,
                            Header(id.clone()),
                            Node {
                                flex_grow: 1.,
                                min_width: px(0.),
                                padding: UiRect::all(px(5.)),
                                ..default()
                            },
                            BackgroundColor(Color::srgb(0.08, 0.18, 0.20)),
                        ))
                        .with_children(|title| {
                            title.spawn((
                                Text::new(name),
                                TextFont {
                                    font_size: 17.,
                                    ..default()
                                },
                                TextColor(Color::srgb(0.25, 1., 0.4)),
                            ));
                        });
                        bar.spawn((
                            Button,
                            Action(id.clone(), Operation::Collapse),
                            Node {
                                width: px(30.),
                                justify_content: JustifyContent::Center,
                                align_items: AlignItems::Center,
                                ..default()
                            },
                            BackgroundColor(Color::srgb(0.12, 0.20, 0.26)),
                        ))
                        .with_children(|b| {
                            b.spawn((
                                CollapseLabel(id.clone()),
                                Text::new("-"),
                                TextFont {
                                    font_size: 18.,
                                    ..default()
                                },
                                TextColor(Color::WHITE),
                            ));
                        });
                    });
                window
                    .spawn((
                        Body(id.clone()),
                        Node {
                            flex_direction: FlexDirection::Column,
                            row_gap: px(6.),
                            ..default()
                        },
                    ))
                    .with_children(|body| {
                        for i in 0..PAGE {
                            body.spawn((
                                Row(id.clone(), i),
                                Node {
                                    align_items: AlignItems::Center,
                                    column_gap: px(4.),
                                    min_height: px(34.),
                                    ..default()
                                },
                                BackgroundColor(Color::srgb(0.08, 0.11, 0.15)),
                            ))
                            .with_children(|row| {
                                row.spawn((
                                    Label(id.clone(), i),
                                    Text::new(""),
                                    TextFont {
                                        font_size: 14.,
                                        ..default()
                                    },
                                    TextColor(Color::WHITE),
                                    Node {
                                        flex_grow: 1.,
                                        width: px(132.),
                                        ..default()
                                    },
                                ));
                                button(row, "-", Action(id.clone(), Operation::Adjust(i, -1)));
                                row.spawn((
                                    ValueLabel(id.clone(), i),
                                    Text::new(""),
                                    TextFont {
                                        font_size: 14.,
                                        ..default()
                                    },
                                    TextColor(Color::WHITE),
                                    Node {
                                        width: px(51.),
                                        ..default()
                                    },
                                ));
                                button(row, "+", Action(id.clone(), Operation::Adjust(i, 1)));
                            });
                        }
                        body.spawn(Node {
                            column_gap: px(6.),
                            ..default()
                        })
                        .with_children(|p| {
                            button(p, "<", Action(id.clone(), Operation::Page(-1)));
                            button(p, ">", Action(id.clone(), Operation::Page(1)));
                            button(p, "Manage mod", Action(id.clone(), Operation::Configure));
                        });
                        body.spawn((
                            Hint(id.clone()),
                            Text::new(""),
                            TextFont {
                                font_size: 12.,
                                ..default()
                            },
                            TextColor(Color::srgb(0.65, 0.85, 0.85)),
                        ));
                    });
            });
    }
}
fn input(
    mut panel: ResMut<EnabledPanel>,
    pause: Res<crate::graphics_menu::Menu>,
    mut menu: ResMut<ModMenu>,
    custom: Res<crate::customiser::Customiser>,
    mut mods: ResMut<Mods>,
    keys: Res<ButtonInput<KeyCode>>,
    mouse: Res<ButtonInput<MouseButton>>,
    nav: Res<crate::customiser::Navigation>,
    window: Single<&Window, With<bevy::window::PrimaryWindow>>,
    headers: Query<(&Interaction, &Header), Changed<Interaction>>,
    buttons: Query<(&Interaction, &Action), Changed<Interaction>>,
) {
    if !pause.open || menu.open || custom.open {
        panel.focused = false;
        panel.drag = None;
        return;
    }
    let ids: Vec<_> = mods
        .manager
        .packages
        .iter()
        .filter(|(_, p)| p.running())
        .map(|(id, _)| id.clone())
        .collect();
    if ids.is_empty() {
        panel.focused = false;
        panel.drag = None;
        return;
    }
    if keys.just_pressed(KeyCode::Tab) || nav.pressed & 0x4000 != 0 {
        let next = if panel.focused {
            panel
                .active
                .as_ref()
                .and_then(|id| ids.iter().position(|s| s == id))
                .map(|i| i + 1)
                .unwrap_or(0)
        } else {
            0
        };
        panel.focused = next < ids.len();
        if panel.focused {
            panel.active = Some(ids[next].clone());
        }
    }
    if !mouse.pressed(MouseButton::Left) {
        panel.drag = None;
    }
    for (interaction, header) in &headers {
        if *interaction == Interaction::Pressed && mouse.pressed(MouseButton::Left) {
            if let (Some(cursor), Some(layout)) =
                (window.cursor_position(), panel.layouts.get(&header.0))
            {
                panel.drag = Some((header.0.clone(), cursor - layout.position));
                panel.active = Some(header.0.clone());
                panel.focused = true;
            }
        }
    }
    if let (Some((id, offset)), Some(cursor)) = (panel.drag.clone(), window.cursor_position()) {
        if let Some(layout) = panel.layouts.get_mut(&id) {
            layout.position = (cursor - offset).clamp(
                Vec2::ZERO,
                Vec2::new(
                    (window.width() - 320.).max(0.),
                    (window.height() - 48.).max(0.),
                ),
            );
        }
    }
    let mut action = None;
    if panel.focused {
        if let Some(id) = panel.active.clone() {
            if let (Some(p), Some(layout)) =
                (mods.manager.packages.get(&id), panel.layouts.get_mut(&id))
            {
                let count = p.manifest.settings.len();
                if count > 0 {
                    layout.selected = layout.selected.min(count - 1);
                    if keys.just_pressed(KeyCode::ArrowUp) || nav.pressed & 1 != 0 {
                        layout.selected = (layout.selected + count - 1) % count;
                    }
                    if keys.just_pressed(KeyCode::ArrowDown) || nav.pressed & 2 != 0 {
                        layout.selected = (layout.selected + 1) % count;
                    }
                    if keys.just_pressed(KeyCode::ArrowLeft) || nav.pressed & 4 != 0 {
                        action = Some(Action(
                            id.clone(),
                            Operation::Adjust(layout.selected % PAGE, -1),
                        ));
                    }
                    if keys.just_pressed(KeyCode::ArrowRight)
                        || keys.just_pressed(KeyCode::Enter)
                        || nav.pressed & (8 | 0x1000) != 0
                    {
                        action = Some(Action(
                            id.clone(),
                            Operation::Adjust(layout.selected % PAGE, 1),
                        ));
                    }
                }
            }
        }
    }
    for (interaction, button) in &buttons {
        if *interaction == Interaction::Pressed {
            action = Some(button.clone());
            panel.active = Some(button.0.clone());
            panel.focused = true;
        }
    }
    let Some(Action(id, operation)) = action else {
        return;
    };
    let Some(p) = mods.manager.packages.get(&id) else {
        return;
    };
    if !p.running() {
        return;
    }
    let Some(layout) = panel.layouts.get_mut(&id) else {
        return;
    };
    match operation {
        Operation::Collapse => {
            layout.collapsed = !layout.collapsed;
        }
        Operation::Configure => menu.configure(id),
        Operation::Page(direction) => {
            let pages = p.manifest.settings.len().max(1).div_ceil(PAGE);
            layout.selected = ((layout.selected / PAGE) as i32 + direction).rem_euclid(pages as i32)
                as usize
                * PAGE;
        }
        Operation::Adjust(row, direction) => {
            if layout.collapsed {
                return;
            }
            let selected = layout.selected / PAGE * PAGE + row;
            let Some((key, s)) = p.manifest.settings.iter().nth(selected) else {
                return;
            };
            let key = key.clone();
            layout.selected = selected;
            let v = &p.settings[&key];
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
                        .position(|c| Some(c.as_str()) == v.as_str())
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
                layout.status = mods.manager.setting(&id, &key, v).err().unwrap_or_default();
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
    window: Single<&Window, With<bevy::window::PrimaryWindow>>,
    mut roots: Query<(&Root, &mut Node, &mut GlobalZIndex)>,
    mut bodies: Query<(&Body, &mut Node), Without<Root>>,
    mut rows: Query<(&Row, &mut Node, &mut BackgroundColor), (Without<Root>, Without<Body>)>,
    mut text: Query<(
        &mut Text,
        Option<&Label>,
        Option<&ValueLabel>,
        Option<&Hint>,
        Option<&CollapseLabel>,
    )>,
) {
    let visible = pause.open && !menu.open && !custom.open;
    for (root, mut node, mut z) in &mut roots {
        if let Some(layout) = panel.layouts.get(&root.0) {
            node.display = if visible
                && mods
                    .manager
                    .packages
                    .get(&root.0)
                    .is_some_and(|p| p.running())
            {
                Display::Flex
            } else {
                Display::None
            };
            node.left = px(layout.position.x.clamp(0., (window.width() - 320.).max(0.)));
            node.top = px(layout.position.y.clamp(0., (window.height() - 48.).max(0.)));
            z.0 = if panel.active.as_ref() == Some(&root.0) {
                14
            } else {
                12
            };
        }
    }
    for (body, mut node) in &mut bodies {
        node.display = if panel.layouts.get(&body.0).is_some_and(|l| !l.collapsed) {
            Display::Flex
        } else {
            Display::None
        };
    }
    for (row, mut node, mut color) in &mut rows {
        let selected = panel.layouts.get(&row.0).map_or(0, |l| l.selected);
        let index = selected / PAGE * PAGE + row.1;
        node.display = if mods
            .manager
            .packages
            .get(&row.0)
            .is_some_and(|p| index < p.manifest.settings.len())
        {
            Display::Flex
        } else {
            Display::None
        };
        color.0 = if panel.focused && panel.active.as_ref() == Some(&row.0) && index == selected {
            Color::srgb(0.10, 0.30, 0.34)
        } else {
            Color::srgb(0.08, 0.11, 0.15)
        };
    }
    for (mut t, label, value, hint, collapse) in &mut text {
        if let Some(c) = collapse {
            **t = if panel.layouts.get(&c.0).is_some_and(|l| l.collapsed) {
                "+"
            } else {
                "-"
            }
            .into();
        }
        if let Some(h) = hint {
            if let Some(l) = panel.layouts.get(&h.0) {
                **t = format!(
                    "Drag title to move | +/- change values\nTab / X cycles windows; arrows adjust\n{}",
                    l.status
                );
            }
        }
        let field = label
            .map(|l| (&l.0, l.1, false))
            .or_else(|| value.map(|v| (&v.0, v.1, true)));
        if let Some((id, row, is_value)) = field {
            let setting = mods
                .manager
                .packages
                .get(id)
                .zip(panel.layouts.get(id))
                .and_then(|(p, l)| {
                    p.manifest
                        .settings
                        .iter()
                        .nth(l.selected / PAGE * PAGE + row)
                        .map(|(k, s)| (p, k, s))
                });
            **t = setting
                .map(|(p, key, s)| {
                    if !is_value {
                        s.label.clone()
                    } else {
                        let v = &p.settings[key];
                        v.as_f64().map(|n| format!("{n:.2}")).unwrap_or_else(|| {
                            v.as_str()
                                .map(str::to_owned)
                                .unwrap_or_else(|| v.to_string())
                        })
                    }
                })
                .unwrap_or_default();
        }
    }
}
