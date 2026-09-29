//! Reversible, owner-scoped local actor suspension; no game-mode rules.
use super::Mods;
use bevy::prelude::*;

pub(crate) fn player_suspended(mods: &Mods) -> bool { !mods.suspended_by.is_empty() }
pub(crate) fn peer_suspended(mods: &Mods, peer: u64) -> bool {
    mods.skater_remote.get(&peer).is_some_and(|o| o.suspended)
}
pub(super) fn clear(world: &mut World, mods: &mut Mods) {
    mods.suspended_by.clear();
    for (entity, visibility) in std::mem::take(&mut mods.hidden_players) {
        if let Some(mut v) = world.get_mut::<Visibility>(entity) { *v = visibility; }
    }
}
pub(super) fn present(world: &mut World) {
    world.resource_scope(|world, mut mods: Mut<Mods>| {
        if player_suspended(&mods) {
            let roots: Vec<_> = world.query_filtered::<Entity, With<crate::world::PlayerRoot>>().iter(world).collect();
            for entity in roots {
                if let Some(mut v) = world.get_mut::<Visibility>(entity) {
                    let original = mods.attach.as_ref().and_then(|a| a.hidden.iter().find(|(e,_)| *e==entity).map(|(_,v)| *v)).unwrap_or(*v);
                    mods.hidden_players.entry(entity).or_insert(original);
                    *v = Visibility::Hidden;
                }
            }
        } else if mods.attach.is_none() {
            clear(world, &mut mods);
        }
    });
}
