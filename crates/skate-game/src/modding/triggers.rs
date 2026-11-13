//! Mod side of the engine's trigger volumes (`crate::trigger_volumes`):
//! `sdk.triggers` snapshot, enter/exit `on_event`s, mod-owned volumes, map
//! volume switches, tracked mod bodies and the query-shape constants. All of
//! it belongs to the mod that set it and goes away with it.
use super::Mods;
use crate::trigger_volumes::{TriggerBodies, TriggerState, TriggerVolume, TriggerVolumes, VolumeSource};
use bevy::prelude::*;
use serde_json::{json, Value};
use skate_core::triggers::{OrientedBox, QueryShape};
use skate_data::trigger_volumes::TriggerGroup;
use std::collections::BTreeMap;

const MAX_PER_MOD: usize = 64;
const MAX_TRACKED_PER_MOD: usize = 16;

/// Mod physics bodies followed by the trigger system: (owner, key) -> options.
#[derive(Resource, Default)]
pub(super) struct TrackedBodies(BTreeMap<(String, String), skate_mods::TriggerTrackOptions>);

pub(super) fn install(app: &mut App) {
    app.init_resource::<TrackedBodies>();
}

fn body_id(owner: &str, key: &str) -> String {
    format!("{}{owner}:{key}", crate::trigger_volumes::MOD_PREFIX)
}

fn hex(value: Option<u64>) -> Value {
    value.map_or(Value::Null, |v| json!(format!("{v:016x}")))
}

fn quat_from_axes(axes: [[f32; 3]; 3]) -> [f32; 4] {
    let m = Mat3::from_cols(Vec3::from_array(axes[0]), Vec3::from_array(axes[1]), Vec3::from_array(axes[2]));
    Quat::from_mat3(&m).normalize().to_array()
}

fn volume_value(v: &TriggerVolume, enabled: bool, inside: Vec<&str>) -> Value {
    let (lo, hi) = v.shape.aabb();
    json!({
        "id": v.id, "name": v.name, "full_name": v.full_name, "group": v.group.as_str(),
        "source": match &v.source { VolumeSource::Map => "map", VolumeSource::Mod(_) => "mod" },
        "owner": match &v.source { VolumeSource::Map => Value::Null, VolumeSource::Mod(o) => json!(o) },
        "instance_id": hex(v.instance_id), "link_guid": hex(v.link_guid),
        "center": v.shape.center, "axes": v.shape.axes, "rotation": quat_from_axes(v.shape.axes),
        "half_extents": v.shape.half_extents, "fatness": v.shape.fatness,
        "aabb_min": lo, "aabb_max": hi, "enabled": enabled, "inside": inside,
    })
}

/// `snapshot.triggers`: every volume (map, then mod) and who is inside what.
pub(super) fn snapshot(world: &World) -> Value {
    let (Some(volumes), Some(state)) = (world.get_resource::<TriggerVolumes>(), world.get_resource::<TriggerState>()) else {
        return json!({"volumes": [], "bodies": {}});
    };
    let inside_of = |id: &str| -> Vec<&str> {
        state.tracker.bodies().filter(|(_, v)| v.iter().any(|x| x == id)).map(|(b, _)| b.as_str()).collect()
    };
    let rows: Vec<Value> = volumes.map.iter().chain(volumes.mods.values())
        .map(|v| volume_value(v, volumes.enabled(&v.id), inside_of(&v.id))).collect();
    let bodies: serde_json::Map<String, Value> = state.tracker.bodies()
        .map(|(b, v)| (b.clone(), json!(v))).collect();
    json!({"volumes": rows, "bodies": bodies})
}

/// Deliver queued enter/exit events as `on_event` to every running mod.
pub(super) fn dispatch(world: &mut World, mods: &mut Mods) {
    if let Some(mut teleporters) = world.get_resource_mut::<crate::teleporters::Teleporters>() {
        for used in std::mem::take(&mut teleporters.mod_queue) {
            mods.manager.dispatch("on_event", json!({"name": "teleporter_used", "teleporter": used.teleporter,
                "exit": used.exit + 1, "body": used.body}));
        }
    }
    let Some(mut state) = world.get_resource_mut::<TriggerState>() else { return };
    let events = std::mem::take(&mut state.mod_queue);
    if events.is_empty() {
        return;
    }
    let volumes = world.resource::<TriggerVolumes>();
    for event in events {
        let volume = volumes.get(&event.volume);
        mods.manager.dispatch("on_event", json!({
            "name": if event.entered { "trigger_entered" } else { "trigger_exited" },
            "body": event.body,
            "volume": event.volume,
            "volume_name": volume.map(|v| v.name.as_str()),
            "group": volume.map(|v| v.group.as_str()),
            "instance_id": hex(volume.and_then(|v| v.instance_id)),
            "link_guid": hex(volume.and_then(|v| v.link_guid)),
        }));
    }
}

pub(super) fn set_box(world: &mut World, owner: &str, key: String, options: skate_mods::TriggerBoxOptions) -> Result<(), String> {
    let mut volumes = world.resource_mut::<TriggerVolumes>();
    let slot = (owner.to_owned(), key.clone());
    if !volumes.mods.contains_key(&slot) && volumes.mods.keys().filter(|(o, _)| o == owner).count() >= MAX_PER_MOD {
        return Err(format!("{MAX_PER_MOD} trigger volumes per mod maximum"));
    }
    let rotation = options.rotation.map(Quat::from_array).unwrap_or(Quat::IDENTITY).normalize();
    let m = Mat3::from_quat(rotation);
    let shape = OrientedBox {
        center: options.center,
        axes: [m.x_axis.to_array(), m.y_axis.to_array(), m.z_axis.to_array()],
        half_extents: options.half_extents,
        fatness: 0.,
    };
    if !shape.is_valid() {
        return Err("invalid trigger box".into());
    }
    let id = TriggerVolumes::mod_id(owner, &key);
    let group = options.group.as_deref().and_then(TriggerGroup::parse).unwrap_or_default();
    volumes.mods.insert(slot, TriggerVolume {
        name: options.name.unwrap_or_else(|| key.clone()), id, full_name: None, group, shape,
        instance_id: None, link_guid: None, source: VolumeSource::Mod(owner.to_owned()),
    });
    Ok(())
}

/// `sdk.triggers.teleporter`: add or replace (options) or remove (none) one of the mod's teleporters on the loaded map.
pub(super) fn set_teleporter(world: &mut World, owner: &str, key: String, options: Option<skate_mods::TeleporterOptions>) -> Result<(), String> {
    let slot = (owner.to_owned(), key.clone());
    let Some(options) = options else {
        world.resource_mut::<crate::teleporters::Teleporters>().mods.remove(&slot);
        return Ok(());
    };
    let map = world.resource::<crate::map_transition::CurrentMap>().path.as_deref()
        .and_then(|p| p.file_stem()).map(|s| s.to_string_lossy().into_owned())
        .ok_or("teleporters need a loaded map")?;
    // The volume: a map volume by id, else the mod's own box by key.
    let volumes = world.resource::<TriggerVolumes>();
    let volume = if volumes.map.iter().any(|v| v.id == options.volume) {
        options.volume.clone()
    } else if volumes.mods.contains_key(&(owner.to_owned(), options.volume.clone())) {
        TriggerVolumes::mod_id(owner, &options.volume)
    } else {
        return Err(format!("no trigger volume {}", options.volume));
    };
    let teleporters = world.resource::<crate::teleporters::Teleporters>();
    if !teleporters.mods.contains_key(&slot) && teleporters.mods.keys().filter(|(o, _)| o == owner).count() >= MAX_PER_MOD {
        return Err(format!("{MAX_PER_MOD} teleporters per mod maximum"));
    }
    let exits = options.exits.iter().map(|e| {
        // Rows: right, up, forward (sin h, 0, cos h), position (as setup's locator matrices).
        let (s, c) = e.heading.sin_cos();
        crate::teleporters::Exit { map: e.map.clone().unwrap_or_else(|| map.clone()), locator: String::new(),
            matrix: [[c, 0., -s, 0.], [0., 1., 0., 0.], [s, 0., c, 0.], [e.position[0], e.position[1], e.position[2], 1.]],
            world: e.world.clone() }
    }).collect();
    // The icon: a `ribbon_indicator` key, upright at the mod's point or the volume's centre.
    let indicator = options.indicator.clone().map(|key| {
        let at = options.indicator_position.or_else(|| volumes_center(world, &volume)).unwrap_or_default();
        crate::visual_indicators::Placement { key, locator: String::new(),
            matrix: [[1., 0., 0., 0.], [0., 1., 0., 0.], [0., 0., 1., 0.], [at[0], at[1], at[2], 1.]],
            style: options.indicator_style.clone(), colour: options.indicator_colour.clone() }
    });
    let teleporter = crate::teleporters::Teleporter { id: key, map, volume_id: volume, volume: None, title_id: None,
        title: options.title, exits, layout: None, indicator };
    world.resource_mut::<crate::teleporters::Teleporters>().mods.insert(slot, teleporter);
    Ok(())
}

fn volumes_center(world: &World, id: &str) -> Option<[f32; 3]> {
    world.resource::<TriggerVolumes>().get(id).map(|v| v.shape.center)
}

pub(super) fn remove_box(world: &mut World, owner: &str, key: &str) {
    world.resource_mut::<TriggerVolumes>().mods.remove(&(owner.to_owned(), key.to_owned()));
}

/// Switch a map volume off (or back on). Only map volumes; the switch is the mod's.
pub(super) fn enable(world: &mut World, owner: &str, id: &str, enabled: bool) -> Result<(), String> {
    let mut volumes = world.resource_mut::<TriggerVolumes>();
    if !volumes.map.iter().any(|v| v.id == id) {
        return Err(format!("no map trigger volume {id:?}"));
    }
    match volumes.disabled.get(id) {
        Some(other) if other != owner => return Err(format!("trigger volume {id} is switched off by {other}")),
        _ => {}
    }
    if enabled {
        volumes.disabled.remove(id);
    } else {
        volumes.disabled.insert(id.to_owned(), owner.to_owned());
    }
    Ok(())
}

pub(super) fn track(world: &mut World, mods: &Mods, owner: &str, key: String, options: Option<skate_mods::TriggerTrackOptions>) -> Result<(), String> {
    if !mods.bodies.contains_key(&(owner.to_owned(), key.clone())) {
        return Err(format!("unknown physics body {key:?}"));
    }
    let mut tracked = world.resource_mut::<TrackedBodies>();
    let slot = (owner.to_owned(), key);
    if !tracked.0.contains_key(&slot) && tracked.0.keys().filter(|(o, _)| o == owner).count() >= MAX_TRACKED_PER_MOD {
        return Err(format!("{MAX_TRACKED_PER_MOD} tracked trigger bodies per mod maximum"));
    }
    tracked.0.insert(slot, options.unwrap_or_default());
    Ok(())
}

pub(super) fn untrack(world: &mut World, owner: &str, key: &str) {
    world.resource_mut::<TrackedBodies>().0.remove(&(owner.to_owned(), key.to_owned()));
    world.resource_mut::<TriggerBodies>().bodies.remove(&body_id(owner, key));
}

pub(super) fn configure(world: &mut World, owner: &str, options: Option<skate_mods::TriggerShapeOptions>) -> Result<(), String> {
    let mut state = world.resource_mut::<TriggerState>();
    if state.shape_owner.as_deref().is_some_and(|o| o != owner) {
        return Err("the trigger query shape is configured by another mod".into());
    }
    let Some(o) = options else {
        state.shape = QueryShape::RETAIL;
        state.shape_owner = None;
        return Ok(());
    };
    let r = QueryShape::RETAIL;
    state.shape = QueryShape {
        radius: o.radius.unwrap_or(r.radius),
        length_scale: o.length_scale.unwrap_or(r.length_scale),
        length_pad: o.length_pad.unwrap_or(r.length_pad),
        foot_pad: o.foot_pad.unwrap_or(r.foot_pad),
    };
    state.shape_owner = Some(owner.to_owned());
    Ok(())
}

/// After the dynamics step: tracked mod bodies' cylinders for the next trigger update.
pub(super) fn sync_tracked(world: &mut World, mods: &Mods) {
    let shape = world.resource::<TriggerState>().shape;
    let mut rows = Vec::new();
    world.resource_mut::<TrackedBodies>().0.retain(|(owner, key), options| {
        let Some(snapshot) = mods.bodies.get(&(owner.clone(), key.clone())).and_then(|id| mods.world.read(*id)) else {
            return false; // the body is gone: stop tracking (its exits follow)
        };
        // The body's position is the feet point; the cylinder stands on it and
        // reaches `length` up (the player's head), axis straight down.
        let feet = snapshot.position;
        let length = options.length.unwrap_or(0.);
        let head = [feet[0], feet[1] + length, feet[2]];
        let hips = [feet[0], feet[1] + 1., feet[2]];
        let query = QueryShape { radius: options.radius.unwrap_or(shape.radius), ..shape };
        if let Some(cylinder) = query.cylinder(feet, head, hips) {
            rows.push((body_id(owner, key), cylinder));
        }
        true
    });
    let mut bodies = world.resource_mut::<TriggerBodies>();
    bodies.bodies.retain(|id, _| !id.starts_with(crate::trigger_volumes::MOD_PREFIX));
    bodies.bodies.extend(rows);
}

/// Everything one mod set: its volumes, switches, tracked bodies, shape.
pub(super) fn clear_owner(world: &mut World, owner: &str) {
    if let Some(mut volumes) = world.get_resource_mut::<TriggerVolumes>() {
        volumes.clear_owner(owner);
    }
    if let Some(mut teleporters) = world.get_resource_mut::<crate::teleporters::Teleporters>() {
        teleporters.clear_owner(owner);
    }
    if let Some(mut tracked) = world.get_resource_mut::<TrackedBodies>() {
        tracked.0.retain(|(o, _), _| o != owner);
    }
    let prefix = body_id(owner, "");
    if let Some(mut bodies) = world.get_resource_mut::<TriggerBodies>() {
        bodies.bodies.retain(|id, _| !id.starts_with(&prefix));
    }
    if let Some(mut state) = world.get_resource_mut::<TriggerState>() {
        if state.shape_owner.as_deref() == Some(owner) {
            state.shape = QueryShape::RETAIL;
            state.shape_owner = None;
        }
    }
}

/// World change / runtime reset: mods re-add their volumes on `world_changed`.
pub(super) fn clear(world: &mut World) {
    if let Some(mut teleporters) = world.get_resource_mut::<crate::teleporters::Teleporters>() {
        teleporters.mods.clear();
    }
    if let Some(mut tracked) = world.get_resource_mut::<TrackedBodies>() {
        tracked.0.clear();
    }
    let owners: Vec<String> = world.get_resource::<TriggerVolumes>()
        .map(|v| v.mods.keys().map(|(o, _)| o.clone()).chain(v.disabled.values().cloned()).collect())
        .unwrap_or_default();
    for owner in owners {
        clear_owner(world, &owner);
    }
    if let Some(mut state) = world.get_resource_mut::<TriggerState>() {
        state.shape = QueryShape::RETAIL;
        state.shape_owner = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::trigger_volumes::TriggerState;

    fn world() -> World {
        let doc = br#"{"format":"skate3rust-trigger-volumes","version":1,"volumes":[
            {"id":"2c7017060025128f","name":"tut_sksc_reset_vol01","instance_id":"2c7017060025128f",
             "link_guid":"a2456e5cb2b3f6ae","shape":{"kind":"box","center":[0,0,0],"half_extents":[100,50,100]}}]}"#;
        let records = skate_data::trigger_volumes::parse(doc).unwrap().volumes;
        let mut world = World::new();
        world.insert_resource(TriggerVolumes::from_records(&records, "test".into()).unwrap());
        world.init_resource::<TriggerBodies>();
        world.init_resource::<TriggerState>();
        world.init_resource::<TrackedBodies>();
        world
    }

    fn options(group: Option<&str>) -> skate_mods::TriggerBoxOptions {
        skate_mods::TriggerBoxOptions { center: [1., 2., 3.], half_extents: [1., 1., 2.],
            rotation: Some(Quat::from_rotation_y(std::f32::consts::FRAC_PI_2).to_array()), name: None, group: group.map(Into::into) }
    }

    #[test]
    fn snapshot_lists_map_and_mod_volumes_with_retail_ids() {
        let mut world = world();
        set_box(&mut world, "demo", "goal".into(), options(Some("stairs"))).unwrap();
        let snap = snapshot(&world);
        let rows = snap["volumes"].as_array().unwrap();
        assert_eq!(rows[0]["name"], "tut_sksc_reset_vol01");
        assert_eq!(rows[0]["instance_id"], "2c7017060025128f");
        assert_eq!(rows[0]["link_guid"], "a2456e5cb2b3f6ae");
        assert_eq!(rows[0]["source"], "map");
        assert_eq!(rows[1]["id"], "mod:demo:goal");
        assert_eq!(rows[1]["group"], "stairs");
        assert_eq!(rows[1]["owner"], "demo");
        // Rotated 90 degrees about Y: the box's x axis points along -z.
        let x = rows[1]["axes"][0].as_array().unwrap().iter().map(|v| v.as_f64().unwrap()).collect::<Vec<_>>();
        assert!(x[0].abs() < 1e-6 && (x[2] + 1.).abs() < 1e-6);
    }

    #[test]
    fn switches_and_shape_belong_to_one_mod_and_end_with_it() {
        let mut world = world();
        enable(&mut world, "demo", "2c7017060025128f", false).unwrap();
        assert!(enable(&mut world, "other", "2c7017060025128f", true).is_err());
        assert!(enable(&mut world, "demo", "nope", false).is_err());
        configure(&mut world, "demo", Some(skate_mods::TriggerShapeOptions { radius: Some(1.), ..Default::default() })).unwrap();
        assert!(configure(&mut world, "other", None).is_err());
        assert_eq!(world.resource::<TriggerState>().shape.radius, 1.);
        set_box(&mut world, "demo", "goal".into(), options(None)).unwrap();
        assert_eq!(snapshot(&world)["volumes"][0]["enabled"], false);
        clear_owner(&mut world, "demo");
        let volumes = world.resource::<TriggerVolumes>();
        assert!(volumes.enabled("2c7017060025128f") && volumes.mods.is_empty());
        assert_eq!(world.resource::<TriggerState>().shape, QueryShape::RETAIL);
    }

    #[test]
    fn mod_volume_limit() {
        let mut world = world();
        for i in 0..MAX_PER_MOD {
            set_box(&mut world, "demo", format!("v{i}"), options(None)).unwrap();
        }
        assert!(set_box(&mut world, "demo", "one-more".into(), options(None)).is_err());
        set_box(&mut world, "demo", "v0".into(), options(None)).unwrap(); // replacing is fine
        remove_box(&mut world, "demo", "v0");
        assert_eq!(world.resource::<TriggerVolumes>().mods.len(), MAX_PER_MOD - 1);
    }
}
