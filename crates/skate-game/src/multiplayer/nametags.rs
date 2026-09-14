//! Labels use the presented remote pose and scale with the output viewport.
use super::{Multiplayer, NetworkActor};
use bevy::prelude::*;
use std::collections::{BTreeMap, BTreeSet};

#[derive(Component)]
pub(crate) struct NameTag(u64);

pub(super) fn draw(
    mut commands: Commands,
    net: Res<Multiplayer>,
    mods: Option<Res<crate::modding::Mods>>,
    cameras: Query<(&Camera, &Transform), With<crate::camera::GameplayCamera>>,
    outputs: Query<(Entity, &Camera), With<IsDefaultUiCamera>>,
    actors: Query<(&NetworkActor, &Transform)>,
    mut tags: Query<(Entity, &NameTag, &mut Text, &mut Node)>,
) {
    let mut labels = BTreeMap::new();
    let output = outputs.single().ok();
    if net.active() {
        if let (Ok((camera, pose)), Some((_, output_camera))) = (cameras.single(), output) {
            if let (Some(view), Some(screen)) = (camera.logical_viewport_size(), output_camera.logical_viewport_size()) {
                if view.x > 0. && view.y > 0. {
                    for (actor, actor_pose) in &actors {
                        if mods.as_ref().is_some_and(|m|crate::modding::peer_suspended(m,actor.0)) {continue;}
                        let point = actor_pose.translation + Vec3::Y * 2.25;
                        if point.distance(pose.translation) > 120. { continue; }
                        if let Ok(pixel) = camera.world_to_viewport(&GlobalTransform::from(*pose), point) {
                            let pixel = pixel / view * screen;
                            if pixel.x >= 0. && pixel.y >= 0. && pixel.x <= screen.x && pixel.y <= screen.y {
                                labels.insert(actor.0, (net.skater_name(&actor.0.to_string(), &net.mod_identity().1.to_string()), pixel));
                            }
                        }
                    }
                }
            }
        }
    }
    let mut existing = BTreeSet::new();
    for (entity, tag, mut text, mut node) in &mut tags {
        if let Some((name, pixel)) = labels.get(&tag.0) {
            text.0.clone_from(name);
            node.left = px(pixel.x - 90.);
            node.top = px(pixel.y - 22.);
            existing.insert(tag.0);
        } else { commands.entity(entity).despawn(); }
    }
    let Some((output, _)) = output else { return; };
    for (id, (name, pixel)) in labels {
        if existing.contains(&id) { continue; }
        commands.spawn((
            NameTag(id), Text::new(name), TextFont { font_size: 18., ..default() },
            TextColor(Color::WHITE), TextLayout::new_with_justify(Justify::Center),
            BackgroundColor(Color::srgba(0.02, 0.03, 0.05, 0.65)),
            UiTargetCamera(output), GlobalZIndex(3),
            Node { position_type: PositionType::Absolute, left: px(pixel.x - 90.), top: px(pixel.y - 22.), width: px(180.), ..default() },
        ));
    }
}
