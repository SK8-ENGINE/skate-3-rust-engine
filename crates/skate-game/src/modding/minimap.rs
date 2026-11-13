//! Mod HUD minimap icons (`sdk.minimap`): one minimap group per mod, cleared with the mod.
//! Details: sdk/ENGINE_API.md "Minimap icons", docs/hails-additions/31-retail-menus.md#hud-minimap
use super::Mods;
use bevy::prelude::*;
use skate_data::challenge_map::MapIcon;
use std::collections::BTreeMap;

const MAX_PER_MOD: usize = 32;

/// The fe_map_icons records, read on the first mod icon.
#[derive(Resource)]
struct MapIcons(BTreeMap<String, MapIcon>);

fn group(owner: &str) -> String {
    format!("mod:{owner}")
}

pub(super) fn set(world: &mut World, mods: &mut Mods, owner: &str, key: String, options: skate_mods::MinimapIconOptions) -> Result<(), String> {
    let slot = (owner.to_owned(), key.clone());
    if !mods.minimap.contains_key(&slot) && mods.minimap.keys().filter(|(o, _)| o == owner).count() >= MAX_PER_MOD {
        return Err("32 minimap icons per mod maximum".into());
    }
    if !world.contains_resource::<MapIcons>() {
        let root = world.resource::<crate::config::Config>().asset_root.clone();
        let icons = skate_data::collections::Collections::load(&root).map(|c| skate_data::challenge_map::map_icons(&c)).unwrap_or_default();
        world.insert_resource(MapIcons(icons));
    }
    let record = skate_data::challenge_map::map_icon(&world.resource::<MapIcons>().0, &options.icon)
        .cloned()
        .ok_or_else(|| format!("no fe_map_icons record {:?}", options.icon))?;
    let icon = skate_core::minimap::Icon {
        id: format!("{}:{key}", group(owner)),
        texture: record.texture,
        position: [options.position[0], options.position[2]],
        angle: options.angle,
        scale: options.scale,
        fade: options.fade,
        max_distance: options.max_distance.unwrap_or(record.max_distance),
        colour: options.colour,
    };
    mods.minimap.insert(slot, icon);
    sync(world, mods, owner);
    Ok(())
}

pub(super) fn remove(world: &mut World, mods: &mut Mods, owner: &str, key: &str) {
    mods.minimap.remove(&(owner.to_owned(), key.to_owned()));
    sync(world, mods, owner);
}

/// The mod's hide of the minimap element, or its end.
pub(super) fn show(world: &mut World, owner: &str, shown: bool) {
    if let Some(mut icons) = world.get_resource_mut::<crate::hud_minimap::MinimapIcons>() {
        icons.set_hidden(&group(owner), !shown);
    }
}

pub(super) fn clear_owner(world: &mut World, mods: &mut Mods, owner: &str) {
    mods.minimap.retain(|(o, _), _| o != owner);
    sync(world, mods, owner);
    show(world, owner, true);
}

pub(super) fn clear(world: &mut World, mods: &mut Mods) {
    let mut owners: Vec<String> = mods.minimap.keys().map(|(o, _)| o.clone()).collect();
    mods.minimap.clear();
    owners.extend(world.get_resource::<crate::hud_minimap::MinimapIcons>().map_or_else(Vec::new, |i| i.hidden_by().filter_map(|g| g.strip_prefix("mod:")).map(str::to_owned).collect()));
    for owner in owners {
        sync(world, mods, &owner);
        show(world, &owner, true);
    }
}

/// The owner's group on the minimap: its icons, or no group once it has none.
fn sync(world: &mut World, mods: &Mods, owner: &str) {
    let Some(mut groups) = world.get_resource_mut::<crate::hud_minimap::MinimapIcons>() else { return };
    let icons: Vec<_> = mods.minimap.iter().filter(|((o, _), _)| o == owner).map(|(_, i)| i.clone()).collect();
    if icons.is_empty() {
        groups.remove(&group(owner));
    } else {
        groups.set(&group(owner), icons);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn icon(id: &str) -> skate_core::minimap::Icon {
        skate_core::minimap::Icon { id: id.into(), texture: "mini_gate".into(), position: [0.0; 2], angle: 0.0, scale: 1.0, fade: true, max_distance: 0.0, colour: [1.0; 4] }
    }

    #[test]
    fn a_mods_icons_form_one_group_that_goes_with_the_mod() {
        let mut world = World::new();
        world.init_resource::<crate::hud_minimap::MinimapIcons>();
        let dir = std::env::temp_dir().join(format!("skate-minimap-mods-{}", std::process::id()));
        let mut mods = Mods::new(skate_mods::Manager::new(dir.clone(), dir));
        mods.minimap.insert(("a".into(), "gate".into()), icon("mod:a:gate"));
        mods.minimap.insert(("b".into(), "gate".into()), icon("mod:b:gate"));
        sync(&mut world, &mods, "a");
        sync(&mut world, &mods, "b");
        let ids = |w: &World| w.resource::<crate::hud_minimap::MinimapIcons>().icons().map(|i| i.id.clone()).collect::<Vec<_>>();
        assert_eq!(ids(&world), ["mod:a:gate", "mod:b:gate"]);
        clear_owner(&mut world, &mut mods, "a");
        assert_eq!(ids(&world), ["mod:b:gate"]);
        show(&mut world, "b", false);
        show(&mut world, "c", false);
        let shown = |w: &World| w.resource::<crate::hud_minimap::MinimapIcons>().element_shown();
        assert!(!shown(&world));
        clear_owner(&mut world, &mut mods, "b");
        assert!(!shown(&world), "another mod still hides it");
        clear(&mut world, &mut mods);
        assert!(ids(&world).is_empty());
        assert!(shown(&world), "no mod hides it after a clear");
    }
}
