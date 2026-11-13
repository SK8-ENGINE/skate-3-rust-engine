//! The alert over a warning (yellow) or chasing (red) ped: retail's ChaseArrow HUD (b191); mods: `sdk.ui.chase_alerts`.
//! Details: docs/hails-additions/26g-living-world-ped-interactions.md#chase-alerts
use super::peds::{PedBody, PedEvent, Pedestrian, PLAYER_TARGET_BASE};
use bevy::{
    asset::RenderAssetUsages,
    prelude::*,
    render::render_resource::{Extent3d, TextureDimension, TextureFormat},
};
use skate_core::living_world::peds::chase_alert::{place, AlertSettings, Alerts, ChaseState, Placement};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Resource)]
pub(crate) struct PedAlerts {
    pub settings: AlertSettings,
    pub alerts: Alerts,
    /// Metres above the ped's root (ours, see the module note).
    pub anchor_height: f32,
    /// A mod that hid the alerts (`sdk.ui.chase_alerts(false)`).
    pub hidden_by: Option<String>,
    textures: BTreeMap<&'static str, Handle<Image>>,
}

impl Default for PedAlerts {
    fn default() -> Self {
        Self { settings: AlertSettings::default(), alerts: Alerts::default(), anchor_height: 2.1, hidden_by: None, textures: BTreeMap::new() }
    }
}

impl PedAlerts {
    /// A mod's switch; `None` releases every mod's hold, `Some(owner)` only that mod's.
    pub(crate) fn clear_owner(&mut self, owner: Option<&str>) {
        if owner.is_none_or(|o| self.hidden_by.as_deref() == Some(o)) {
            self.hidden_by = None;
        }
    }
}

#[derive(Component)]
pub(crate) struct PedAlertNode(u64);

/// Retail's four state textures (manifest `chase_arrows.json`); without them the alerts are tracked but not drawn.
pub(crate) fn load_alert_textures(config: Res<crate::config::Config>, mut alerts: ResMut<PedAlerts>, mut images: ResMut<Assets<Image>>) {
    let root = config.asset_root.join("private/hud/chase-arrows");
    let Some(manifest) = std::fs::read(root.join("chase_arrows.json")).ok().and_then(|b| serde_json::from_slice::<serde_json::Value>(&b).ok()) else {
        info!("PED_ALERTS textures not installed ({}): Refresh Game Files adds them", root.display());
        return;
    };
    for state in [ChaseState::Warn, ChaseState::Chase, ChaseState::Tired, ChaseState::Giveup] {
        let t = &manifest["textures"][state.key()];
        let (Some(file), Some(w), Some(h)) = (t["file"].as_str(), t["width"].as_u64(), t["height"].as_u64()) else { continue };
        match std::fs::read(root.join(file)) {
            Ok(bytes) if bytes.len() as u64 == w * h * 4 => {
                let image = Image::new(
                    Extent3d { width: w as u32, height: h as u32, depth_or_array_layers: 1 },
                    TextureDimension::D2,
                    bytes,
                    TextureFormat::Rgba8UnormSrgb,
                    RenderAssetUsages::RENDER_WORLD,
                );
                alerts.textures.insert(state.key(), images.add(image));
            }
            _ => warn!("PED_ALERTS texture {file} missing or the wrong size"),
        }
    }
}

/// The state messages for the local player, then one fixed step of aging (`825E35F0`, `825E2738`).
pub(crate) fn update_ped_alerts(
    mut alerts: ResMut<PedAlerts>,
    mut events: MessageReader<PedEvent>,
    peds: Query<&Pedestrian>,
    time: Res<Time>,
) {
    let alerts = &mut *alerts;
    for event in events.read() {
        let PedEvent::Chase { id, chasee, kind, .. } = event else { continue };
        let Some(state) = kind.strip_prefix("state_") else { continue };
        // Local play: player 0 (retail draws arrows for the local player's chasers).
        if *chasee != PLAYER_TARGET_BASE {
            continue;
        }
        let state = ChaseState::from_name(state);
        alerts.alerts.post(&alerts.settings, id.to_u64(), state);
        info!("PED_ALERT ped=#{} {}", id.serial, state.key());
    }
    let alive: BTreeSet<u64> = peds.iter().map(|p| p.id.to_u64()).collect();
    let ms = (time.delta_secs() * 1000.0).round() as u32;
    alerts.alerts.tick(&alerts.settings, ms, |ped| alive.contains(&ped));
}

/// One image per alert: over the ped inside the centred box, else an edge arrow pointing at it (`825E2B80`).
#[allow(clippy::too_many_arguments)]
pub(crate) fn draw_ped_alerts(
    mut commands: Commands,
    alerts: Res<PedAlerts>,
    cameras: Query<(&Camera, &GlobalTransform), With<crate::camera::GameplayCamera>>,
    outputs: Query<(Entity, &Camera), With<IsDefaultUiCamera>>,
    peds: Query<(&Pedestrian, &PedBody)>,
    mut nodes: Query<(Entity, &PedAlertNode, &mut ImageNode, &mut Node, &mut UiTransform)>,
    fe_hud: Option<Res<crate::fe_backdrop::FeHud>>,
    overlay: Option<Res<crate::graphics_menu::Menu>>,
    replay: Option<Res<crate::replay::Replay>>,
) {
    let mut shown = BTreeMap::new();
    // Gameplay HUD: off while the instant replay is open (laptop session 2026-10-20: the warning stayed over the ped).
    let hidden = alerts.hidden_by.is_some() || fe_hud.is_some_and(|f| !f.hud) || overlay.is_some_and(|m| m.overlay_visible())
        || replay.is_some_and(|r| r.active);
    let output = outputs.single().ok();
    if let (false, Ok((camera, pose)), Some((_, output_camera))) = (hidden, cameras.single(), output) {
        if let (Some(view), Some(screen)) = (camera.logical_viewport_size(), output_camera.logical_viewport_size()) {
            let positions: BTreeMap<u64, Vec3> = peds.iter().map(|(p, b)| (p.id.to_u64(), b.position)).collect();
            for entry in &alerts.alerts.entries {
                let (Some(texture), Some(position)) = (alerts.textures.get(entry.state.key()), positions.get(&entry.ped)) else { continue };
                let point = *position + Vec3::Y * alerts.anchor_height;
                let local = pose.affine().inverse().transform_point3(point);
                // In front: the projection (logical px of the output); behind: the side the ped is on.
                let projected = (local.z < 0.0).then(|| camera.world_to_viewport(pose, point).ok()).flatten().map(|p| p / view * screen);
                let behind = [local.x, -local.y];
                let placement = place(&alerts.settings, projected.map(|p| [p.x, p.y]), behind, [screen.x, screen.y], local.length());
                shown.insert(entry.ped, (texture.clone(), placement));
            }
        }
    }
    let size = alerts.settings.size;
    let layout = |placement: &Placement| -> (Val, Val, f32, f32) {
        match *placement {
            // The sprite's bottom centre on the anchor, pointing down at the ped.
            Placement::Over { at, scale } => (px(at[0] - size * scale * 0.5), px(at[1] - size * scale), size * scale, 0.0),
            // Centred on the ring point, turned from pointing down to pointing at the ped.
            Placement::Edge { at, angle, scale } => (px(at[0] - size * scale * 0.5), px(at[1] - size * scale * 0.5), size * scale, angle - std::f32::consts::PI),
        }
    };
    let mut existing = BTreeSet::new();
    for (entity, alert, mut image, mut node, mut transform) in &mut nodes {
        if let Some((texture, placement)) = shown.get(&alert.0) {
            let (left, top, side, rotation) = layout(placement);
            image.image = texture.clone();
            node.left = left;
            node.top = top;
            node.width = px(side);
            node.height = px(side);
            transform.rotation = Rot2::radians(rotation);
            existing.insert(alert.0);
        } else {
            commands.entity(entity).despawn();
        }
    }
    let Some((output, _)) = output else { return };
    for (ped, (texture, placement)) in shown {
        if existing.contains(&ped) {
            continue;
        }
        let (left, top, side, rotation) = layout(&placement);
        commands.spawn((
            PedAlertNode(ped),
            ImageNode::new(texture),
            UiTargetCamera(output),
            GlobalZIndex(2),
            Pickable::IGNORE,
            UiTransform { rotation: Rot2::radians(rotation), ..default() },
            Node { position_type: PositionType::Absolute, left, top, width: px(side), height: px(side), ..default() },
        ));
    }
}
