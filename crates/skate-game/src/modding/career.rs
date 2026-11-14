//! Mod entry for the career (`sdk.career`): counters, milestones, armed actions, flags, team and achievements, plus
//! board sales / money / action / flag / achievement changes. Career changes stay with the career when the mod stops
//! (they fired graphs that cannot be undone); a mod's own flags are namespaced `<mod id>:<name>`.
//! Details: sdk/ENGINE_API.md "Career", docs/hails-additions/43-career.md
use super::Mods;
use crate::career::CareerState;
use bevy::prelude::*;
use serde_json::{json, Value};

pub(super) fn snapshot(world: &World) -> Value {
    let Some(state) = world.get_resource::<CareerState>() else { return json!({"on": false}) };
    let Some(data) = state.data.as_ref() else { return json!({"on": false}) };
    let c = &state.career;
    let milestones = skate_core::career::milestones::milestones(&data.progression);
    let facts = skate_core::career::milestones::facts(c, &data.progression);
    json!({
        "on": true,
        "board_sales": c.counters.board_sales(),
        "money": c.counters.money,
        "money_earned": c.counters.money_earned,
        "milestone": {"index": facts[0], "count": milestones.len(), "start": facts[2], "goal": facts[3],
            "thresholds": milestones.iter().map(|m| m.1).collect::<Vec<_>>(), "pending": c.pending.milestones},
        "armed": c.tracker.armed,
        "fired": c.tracker.fired,
        "flags": c.runner.flags,
        "team": {"name": c.team.name, "logo": c.team.logo, "members": c.team.members(), "tiers": c.team.tiers,
            "teammates": c.team.teammates.iter().enumerate().map(|(i, t)| json!({"joined": t.joined, "name": t.name,
                "character": t.character, "look": state.looks.look(i + 1).map(|l| &l.source)}))
                .collect::<Vec<_>>()},
        "unlocks": c.unlocks.granted.iter().map(|((kind, name), new)| json!({"kind": kind, "name": name, "new": new})).collect::<Vec<_>>(),
        "unlock_all": c.unlocks.unlock_all,
        "presets": state.presets.iter().map(|p| json!({"key": p.row.key, "id": p.row.id, "name_text": p.row.name_text})).collect::<Vec<_>>(),
        "active": c.active,
        "achievements": state.awards.achievements.granted.keys().collect::<Vec<_>>(),
        "graph": c.runner.running.as_ref().map(|r| json!({"id": r.graph, "state": r.state})),
        "queue": c.runner.queue,
    })
}

/// Queued career events as `on_event` to every running mod.
pub(super) fn dispatch(world: &mut World, mods: &mut Mods) {
    let Some(mut state) = world.get_resource_mut::<CareerState>() else { return };
    for event in std::mem::take(&mut state.awards.events) {
        mods.manager.dispatch("on_event", event);
    }
}

/// Runs a change on the loaded career and marks it for the save and the menus.
fn change(world: &mut World, f: impl FnOnce(&mut CareerState, &skate_data::progression::Loaded) -> Result<(), String>)
    -> Result<(), String> {
    let mut res = world.get_resource_mut::<CareerState>().ok_or("the career is not ready")?;
    let state: &mut CareerState = &mut res;
    let data = state.data.take().ok_or("no career (the Challenges opt-in is off or progression.json is missing)")?;
    let result = f(state, &data);
    state.data = Some(data);
    state.dirty = true;
    result
}

pub(super) fn add_sales(world: &mut World, owner: &str, amount: u64, reason: Option<String>) -> Result<(), String> {
    change(world, |state, data| {
        info!("SKATE_CAREER mod {owner} add_sales {amount} reason={reason:?}");
        let effects = state.career.add_sales(amount, &data.progression);
        crate::career::apply_effects(state, data, effects);
        Ok(())
    })
}

pub(super) fn add_money(world: &mut World, owner: &str, amount: u64) -> Result<(), String> {
    change(world, |state, data| {
        info!("SKATE_CAREER mod {owner} add_money {amount}");
        let effects = state.career.add_money(amount, &data.progression);
        crate::career::apply_effects(state, data, effects);
        Ok(())
    })
}

pub(super) fn arm_action(world: &mut World, owner: &str, id: &str) -> Result<(), String> {
    change(world, |state, data| {
        if !data.progression.actions.contains_key(id) {
            return Err(format!("no progression action {id:?}"));
        }
        info!("SKATE_CAREER mod {owner} arm_action {id}");
        let effects = state.career.arm(id, &data.progression);
        crate::career::apply_effects(state, data, effects);
        Ok(())
    })
}

/// A flag the career already has (a retail one) is set as is; any other goes under the mod's namespace.
pub(super) fn set_flag(world: &mut World, owner: &str, name: &str, value: bool) -> Result<(), String> {
    change(world, |state, _| {
        let key = if state.career.runner.flags.contains_key(name) { name.to_owned() } else { format!("{owner}:{name}") };
        info!("SKATE_CAREER mod {owner} set_flag {key}={value}");
        state.career.runner.flags.insert(key, value);
        Ok(())
    })
}

pub(super) fn grant_achievement(world: &mut World, owner: &str, name: &str) -> Result<(), String> {
    change(world, |state, _| {
        if !state.awards.rows.contains_key(name) {
            return Err(format!("no achievement row {name:?}"));
        }
        info!("SKATE_CAREER mod {owner} grant_achievement {name}");
        state.awards.grant(name);
        Ok(())
    })
}

pub(super) fn grant_unlock(world: &mut World, owner: &str, kind: u32, name: &str) -> Result<(), String> {
    change(world, |state, _| {
        if !state.career.unlocks.grant(kind, name) {
            return Ok(());
        }
        info!("SKATE_CAREER mod {owner} grant_unlock kind={kind} {name}");
        state.awards.event(json!({"name": "career_unlock", "kind": kind, "unlock": name}));
        Ok(())
    })
}

/// A progression reward row by id (its unlock plus the reward summary entry, `big` = the big-reward list).
pub(super) fn grant_reward(world: &mut World, owner: &str, id: &str, big: bool) -> Result<(), String> {
    change(world, |state, data| {
        if !data.rewards.contains_key(id) {
            return Err(format!("no progression reward {id:?}"));
        }
        info!("SKATE_CAREER mod {owner} grant_reward {id} big={big}");
        let s = &mut *state;
        crate::career::grant_reward(&mut s.career, &mut s.awards, data, id, big);
        Ok(())
    })
}

/// Activates a challenge, or every member of a progression group, as the graphs' ActivateChallenge / ActivateGroup do.
pub(super) fn activate(world: &mut World, owner: &str, id: &str, group: bool) -> Result<(), String> {
    change(world, |state, data| {
        let groups = &data.progression.groups;
        let ids: Vec<String> = if group {
            groups.get(id).cloned().ok_or_else(|| format!("no progression group {id:?}"))?
        } else if groups.values().flatten().any(|c| c == id) || data.progression.challenge_rewards.contains_key(id) {
            vec![id.to_owned()]
        } else {
            return Err(format!("no challenge {id:?} in the progression data"));
        };
        info!("SKATE_CAREER mod {owner} activate {id} ({} challenges)", ids.len());
        state.career.active.extend(ids);
        Ok(())
    })
}

pub(super) fn skip_start(world: &mut World, owner: &str) -> Result<(), String> {
    change(world, |state, data| {
        let n = crate::career::skip_start(state, data);
        info!("SKATE_CAREER mod {owner} skip_start ({n} gate challenges completed)");
        Ok(())
    })
}

/// The team's name (`n` None) or teammate n's; kept to 16 characters.
pub(super) fn team_name(world: &mut World, owner: &str, n: Option<usize>, name: &str) -> Result<(), String> {
    change(world, |state, _| {
        let name = skate_core::career::team::clip_name(name);
        let event = match n {
            None => {
                state.career.team.name.clone_from(&name);
                json!({"name": "career_team_name", "team_name": name})
            }
            Some(n) => {
                state.career.team.teammate_mut(n).ok_or(format!("no teammate {n}"))?.name.clone_from(&name);
                json!({"name": "career_teammate_name", "n": n, "teammate_name": name})
            }
        };
        info!("SKATE_CAREER mod {owner} team_name {n:?} {name:?}");
        state.awards.event(event);
        Ok(())
    })
}

/// Teammate n gets a preset's look (an engine-owned look: it stays when the mod stops).
pub(super) fn teammate_preset(world: &mut World, owner: &str, n: usize, key: &str) -> Result<(), String> {
    change(world, |state, _| {
        state.apply_preset(n, key)?;
        info!("SKATE_CAREER mod {owner} teammate_preset {n} {key}");
        state.awards.event(json!({"name": "career_teammate_look", "n": n, "set": true, "preset": key}));
        Ok(())
    })
}

/// Teammate n's look from a mod (None clears it). Mod looks go when the mod stops (`clear_owner`).
pub(super) fn teammate_look(world: &mut World, owner: &str, n: usize, profile: Option<serde_json::Map<String, Value>>)
    -> Result<(), String> {
    change(world, |state, _| {
        if state.career.team.teammate_mut(n).is_none() {
            return Err(format!("no teammate {n}"));
        }
        use skate_data::cac_save::{LookSource, SlotLook};
        let set = profile.as_ref().is_some_and(|p| !p.is_empty());
        state.looks.set(n, profile.map(|profile| SlotLook { source: LookSource::Mod { id: owner.to_owned() }, profile,
            extra: Default::default() }));
        info!("SKATE_CAREER mod {owner} teammate_look {n} set={set}");
        state.awards.event(json!({"name": "career_teammate_look", "n": n, "set": set}));
        state.save_looks();
        Ok(())
    })
}

/// The running mods' progression overlays (`progression/*.json`, b887 §8), in mod id order; read again only when
/// the set of running mods or their content changes. The career rebuilds its data on its next update.
pub(super) fn sync_content(world: &mut World, mods: &Mods) {
    let Some(mut res) = world.get_resource_mut::<CareerState>() else { return };
    let state = res.bypass_change_detection();
    let running: Vec<(String, std::path::PathBuf, u64)> = mods.manager.packages.iter().filter(|(_, p)| p.running())
        .map(|(id, p)| (id.clone(), p.root.clone(), p.content_fingerprint())).collect();
    let keys: Vec<(String, u64)> = running.iter().map(|(id, _, f)| (id.clone(), *f)).collect();
    if keys == state.overlay_keys {
        return;
    }
    state.overlay_keys = keys;
    let overlays: Vec<(String, Vec<Value>)> = running.iter().filter_map(|(id, root, _)| {
        match skate_data::progression::read_overlays(root) {
            Ok(list) if !list.is_empty() => Some((id.clone(), list)),
            Ok(_) => None,
            Err(e) => {
                warn!("SKATE_CAREER mod {id} progression overlay unreadable: {e}");
                None
            }
        }
    }).collect();
    if overlays != state.overlays {
        state.overlays = overlays;
        state.overlays_changed = true;
    }
}

/// Every mod stopped: all namespaced flags go (retail flag names have no `:`).
pub(super) fn clear(world: &mut World) {
    let Some(mut state) = world.get_resource_mut::<CareerState>() else { return };
    let before = state.career.runner.flags.len();
    state.career.runner.flags.retain(|k, _| !k.contains(':'));
    let owners: Vec<String> = state.looks.slots.iter().flatten()
        .filter_map(|l| match &l.source { skate_data::cac_save::LookSource::Mod { id } => Some(id.clone()), _ => None }).collect();
    if owners.iter().fold(false, |any, id| !state.looks.clear_mod(id).is_empty() || any) {
        state.save_looks();
    }
    if state.career.runner.flags.len() != before {
        state.dirty = true;
    }
}

/// A stopped mod's namespaced flags go; its other career changes stay (see the module doc).
pub(super) fn clear_owner(world: &mut World, owner: &str) {
    let Some(mut state) = world.get_resource_mut::<CareerState>() else { return };
    let prefix = format!("{owner}:");
    let before = state.career.runner.flags.len();
    state.career.runner.flags.retain(|k, _| !k.starts_with(&prefix));
    if !state.looks.clear_mod(owner).is_empty() {
        state.save_looks();
    }
    if state.career.runner.flags.len() != before {
        state.dirty = true;
    }
}
