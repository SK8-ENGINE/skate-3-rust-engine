//! Mod entry for the retail challenges (`sdk.challenges`): the map's challenges with their records, the run going,
//! start / abort, hiding a challenge and value overrides per mod, ended with the mod.
//! Details: sdk/ENGINE_API.md "Challenges", docs/hails-additions/42-challenges.md#mod-entry
use super::Mods;
use crate::challenges::{ChallengeSignUp, Challenges};
use bevy::prelude::*;
use serde_json::{json, Value};

pub(super) fn snapshot(world: &World) -> Value {
    let (Some(challenges), Some(map)) = (world.get_resource::<Challenges>(), world.get_resource::<crate::map_transition::CurrentMap>()) else {
        return json!({"on": false, "list": [], "run": null});
    };
    let list: Vec<Value> = challenges.on_map(map.path.as_deref()).map(|c| {
        let c = challenges.mods.apply(c.clone());
        let saved = challenges.records.get(&c.id);
        json!({
            "id": c.id, "family": c.family, "map": c.map, "title_id": c.title_id, "title": c.title,
            "position": &c.location.matrix[3][..3],
            "enabled": challenges.mods.enabled(&c.id),
            "owned_points": c.owned_points, "killed_points": c.killed_points,
            "completion": saved.completion, "high_score": saved.high_score, "discovery": saved.discovery,
        })
    }).collect();
    let run = challenges.run.as_ref().map(|r| json!({
        "id": r.challenge.id,
        "points": r.objectives.points,
        "requirement": r.objectives.requirement,
        "personal_best": r.ots.personal_best(),
        "out_of_bounds": r.ots.out_of_bounds_ticks().map(|t| t as f32 / skate_core::challenges::timers::TICKS_PER_SECOND),
    }));
    let on = world.get_resource::<crate::living_world::LivingWorldSettings>().is_some_and(|l| l.user_opt_ins.challenges);
    json!({"on": on, "list": list, "run": run})
}

/// Queued challenge events as `on_event` to every running mod.
pub(super) fn dispatch(world: &mut World, mods: &mut Mods) {
    let Some(mut challenges) = world.get_resource_mut::<Challenges>() else { return };
    for event in std::mem::take(&mut challenges.mods.events) {
        mods.manager.dispatch("on_event", event);
    }
}

/// Signs the player up as the D-pad press does; the next update starts the run.
pub(super) fn start(world: &mut World, id: &str) -> Result<(), String> {
    if !world.get_resource::<crate::living_world::LivingWorldSettings>().is_some_and(|l| l.user_opt_ins.challenges) {
        return Err("challenges are switched off (Opt-Ins: Challenges)".into());
    }
    let map = world.resource::<crate::map_transition::CurrentMap>().path.clone();
    let challenges = world.get_resource::<Challenges>().ok_or("challenges are not ready")?;
    if let Some(run) = &challenges.run {
        return Err(format!("challenge {} is running", run.challenge.id));
    }
    if !challenges.on_map(map.as_deref()).any(|c| c.id == id) {
        return Err(format!("no challenge {id:?} on this map"));
    }
    world.write_message(ChallengeSignUp { body: crate::trigger_volumes::PLAYER.into(), challenge: id.to_owned() });
    Ok(())
}

pub(super) fn abort(world: &mut World) -> Result<(), String> {
    let mut challenges = world.get_resource_mut::<Challenges>().ok_or("challenges are not ready")?;
    if challenges.run.is_none() {
        return Err("no challenge is running".into());
    }
    challenges.mods.abort = true;
    Ok(())
}

pub(super) fn enable(world: &mut World, owner: &str, id: &str, enabled: bool) -> Result<(), String> {
    let mut challenges = world.get_resource_mut::<Challenges>().ok_or("challenges are not ready")?;
    let owners = challenges.mods.disabled.entry(id.to_owned()).or_default();
    if enabled {
        owners.remove(owner);
    } else {
        owners.insert(owner.to_owned());
    }
    challenges.mods.disabled.retain(|_, o| !o.is_empty());
    Ok(())
}

/// Overrides apply from the next start; a run going keeps the values it started with.
pub(super) fn configure(world: &mut World, owner: &str, id: String, options: Option<skate_mods::ChallengeOptions>) -> Result<(), String> {
    let mut challenges = world.get_resource_mut::<Challenges>().ok_or("challenges are not ready")?;
    match options {
        Some(o) => { challenges.mods.overrides.insert((owner.to_owned(), id), o); }
        None => { challenges.mods.overrides.remove(&(owner.to_owned(), id)); }
    }
    Ok(())
}

pub(super) fn clear_owner(world: &mut World, owner: &str) {
    if let Some(mut challenges) = world.get_resource_mut::<Challenges>() {
        challenges.mods.clear_owner(owner);
    }
}

pub(super) fn clear(world: &mut World) {
    if let Some(mut challenges) = world.get_resource_mut::<Challenges>() {
        challenges.mods = crate::challenges::ModControl::default();
    }
}

#[cfg(test)]
mod tests {
    use crate::challenges::ModControl;

    #[test]
    fn overrides_and_hiding_belong_to_one_mod_and_end_with_it() {
        let mut m = ModControl::default();
        m.disabled.entry("ots_a".into()).or_default().insert("one".into());
        m.disabled.entry("ots_a".into()).or_default().insert("two".into());
        m.overrides.insert(("one".into(), "ots_a".into()), skate_mods::ChallengeOptions { owned_points: Some(10), ..Default::default() });
        m.clear_owner("one");
        assert!(!m.enabled("ots_a"));
        assert!(m.overrides.is_empty());
        m.clear_owner("two");
        assert!(m.enabled("ots_a"));
    }

    #[test]
    fn events_keep_the_newest() {
        let mut m = ModControl::default();
        for i in 0..100 {
            m.event(serde_json::json!(i));
        }
        assert_eq!(m.events.len(), 64);
        assert_eq!(m.events[0], serde_json::json!(36));
    }
}
