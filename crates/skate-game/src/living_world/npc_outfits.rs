//! Teammate stand-ins wear their save slot's look (research b977: retail spawns `teammate_0n` with save skater slot n):
//! the customiser profile from the career's `cac.json` is assembled from the customiser's parts under one container,
//! then bound to the stock skeleton like any NPC look. Other NPCs keep `present_looks`.
//! Details: docs/hails-additions/43-career.md#change-team-brand-tiers-teammates-and-achievements-step-11
use super::npc_skaters::{NpcPuppet, NpcSkater, SaveSlotLooks};
use super::PopulationState;
use crate::customiser_material::SkaterMaterial;
use bevy::prelude::*;

/// The NPC wears this resolved customiser profile (set before its puppet is built).
#[derive(Component)]
pub(crate) struct NpcOutfit {
    profile: serde_json::Value,
    pieces: Vec<(Entity, String, String)>,
}

fn slot_look<'a>(state: &PopulationState, looks: &'a SaveSlotLooks, character: &str) -> Option<&'a serde_json::Value> {
    let slot = state.skaters.as_ref()?.characters.iter().find(|c| c.key == character)?.save_slot?;
    looks.0.get(&slot)
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn present_outfits(
    mut commands: Commands,
    server: Res<AssetServer>,
    state: Res<PopulationState>,
    looks: Option<Res<SaveSlotLooks>>,
    parts: Option<ResMut<crate::customiser_parts::Parts>>,
    materials: Option<ResMut<Assets<SkaterMaterial>>>,
    skater: Res<crate::physics::SkaterRuntime>,
    mut npcs: Query<(Entity, &NpcSkater, Option<&NpcOutfit>, Option<&mut NpcPuppet>)>,
    skins: Query<(Entity, &bevy::mesh::skinning::SkinnedMesh)>,
    nodes: Query<(&Name, &Transform)>,
    parents: Query<&ChildOf>,
    instances: Query<&bevy::scene::SceneInstance>,
    spawner: Res<SceneSpawner>,
    mut morphs: Query<(Entity, &mut bevy::mesh::morph::MorphWeights)>,
) {
    let (Some(looks), Some(mut parts), Some(mut materials)) = (looks, parts, materials) else { return };
    for (e, npc, outfit, puppet) in &mut npcs {
        let Some(outfit) = outfit else {
            if puppet.is_some() {
                continue;
            }
            let Some(profile) = slot_look(&state, &looks, &npc.character) else { continue };
            let profile = match parts.resolve(profile) {
                Ok(p) => p,
                Err(err) => {
                    warn!("LIVING_WORLD teammate look rejected ({}): {err}; stock look", npc.character);
                    continue;
                }
            };
            let container = commands.spawn((Transform::default(), Visibility::Hidden, ChildOf(e))).id();
            let mut pieces = Vec::new();
            for v in profile["selections"].as_object().into_iter().flat_map(|s| s.values()) {
                let Some((id, mid)) = v["asset_id"].as_str().zip(v["material_id"].as_str()) else { continue };
                let Some(part) = parts.library.models.get(id) else { continue };
                let scene = server.load(GltfAssetLabel::Scene(0).from_asset(part.scene.clone()));
                let piece = commands.spawn((SceneRoot(scene), crate::customiser_parts::PartRoot(id.to_owned()), ChildOf(container))).id();
                pieces.push((piece, id.to_owned(), mid.to_owned()));
            }
            commands.entity(e).insert((NpcOutfit { profile, pieces }, NpcPuppet { scene: Some(container), bindings: None, style: crate::custom_models::native_animation_style(&npc.character) }));
            continue;
        };
        let Some(mut puppet) = puppet else { continue };
        let Some(container) = puppet.scene else { continue };
        if puppet.bindings.is_some() {
            continue;
        }
        for (_, _, mid) in &outfit.pieces {
            parts.warm(mid, &server, &mut materials);
        }
        let ready = outfit.pieces.iter().all(|(piece, _, mid)| {
            instances.get(*piece).is_ok_and(|i| spawner.instance_is_ready(**i)) && parts.material_ready(mid, &server)
        }) && parts.tattoos_ready(&outfit.profile, &server);
        if !ready {
            continue;
        }
        let bindings = match crate::animation::AnimationStatus::for_scene(container, &skater.animation.evaluator.frames.bone_names, &skins, &nodes, &parents) {
            Ok(b) => b,
            Err(err) => {
                warn!("LIVING_WORLD teammate look rejected ({}): {err}", npc.character);
                commands.entity(container).despawn();
                puppet.scene = None;
                continue;
            }
        };
        for (piece, id, mid) in &outfit.pieces {
            let Some(material) = parts.profile_material(id, mid, &outfit.profile, &materials) else { continue };
            let handle = materials.add(material);
            for (mesh, _) in &skins {
                if parents.iter_ancestors(mesh).any(|p| p == *piece) {
                    commands.entity(mesh).remove::<MeshMaterial3d<StandardMaterial>>().insert(MeshMaterial3d(handle.clone()));
                }
            }
        }
        let weights: Vec<f32> = parts.library.morphs.iter().enumerate()
            .map(|(i, n)| outfit.profile["morphs"][n].as_f64().unwrap_or(if (2..19).contains(&i) { 0.25 } else { 0.0 }).clamp(0.0, 0.5) as f32)
            .collect();
        for (mesh, mut m) in &mut morphs {
            if parents.iter_ancestors(mesh).any(|p| p == container) && m.weights().len() == weights.len() {
                m.weights_mut().copy_from_slice(&weights);
            }
        }
        for (mesh, _) in &skins {
            if parents.iter_ancestors(mesh).any(|p| p == container) {
                commands.entity(mesh).insert((bevy::camera::visibility::NoFrustumCulling, bevy::camera::visibility::RenderLayers::from_layers(&[0, 28])));
            }
        }
        commands.entity(container).insert(Visibility::Inherited);
        info!("LIVING_WORLD teammate look bound #{} {} ({} parts)", npc.id.serial, npc.character, outfit.pieces.len());
        puppet.bindings = Some(bindings);
    }
}
