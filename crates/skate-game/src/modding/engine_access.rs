//! Read-only structured access to native systems; catalogs are requested on demand.
use crate::{
    graph_runtime::{LoadedGraph, StockGraphs},
    physics::SkaterRuntime,
};
use bevy::prelude::*;
use serde_json::{Value, json};
fn graph_state(c: &skate_core::graph::controller::Controller, g: Option<&LoadedGraph>) -> Value {
    json!({"current":c.frame.current,"previous":c.frame.last,"dt":c.frame.dt,
        "name":c.frame.current.and_then(|id|g.and_then(|g|g.binding.states.get(id)).map(|s|&s.name)),
        "state_times":c.frame.state_times,"active_behaviors":c.active.iter().map(|b|b.behavior).collect::<Vec<_>>()})
}
pub(super) fn snapshot(world: &World) -> Value {
    let s = world.resource::<SkaterRuntime>();
    let graphs = world.get_resource::<StockGraphs>();
    json!({"graphs":{"tick":s.animation.ticks,"action":graph_state(&s.animation.action_controller,graphs.map(|g|&g.action)),
        "motion":graph_state(&s.animation.motion_controller,graphs.map(|g|&g.motion))},
        "animation":{"tick":s.animation.ticks,"pose_generation":s.pose_generation,"bone_count":s.animation.pose.len()},
        "systems":["player","rig","bodies","input","graphs","animation","scoring","world","camera","network","commands"]})
}
fn graph(g: &LoadedGraph) -> Value {
    json!({"behaviors":g.runtime.program.behaviors.iter().enumerate().map(|(id,b)|json!({"id":id,"enabled":b.enabled,"operation_id":g.runtime.operations.behaviors.get(id)})).collect::<Vec<_>>(),"behavior_operation_ids":g.runtime.operations.behaviors,"states":g.binding.states.iter().enumerate().map(|(id,s)|json!({"id":id,"name":s.name,"parent":s.parent,"children":s.children,"behaviors":s.behaviors,"transitions":s.transitions,"authored_enabled":s.enabled!=0,"enabled":g.runtime.program.topology.states.get(id).map(|s|s.enabled)})).collect::<Vec<_>>(),
        "transitions":g.binding.transitions.iter().enumerate().map(|(id,t)|json!({"id":id,"owner":t.owner,"target":t.target,"priority":t.priority,"authored_enabled":t.enabled!=0,"enabled":g.runtime.program.topology.transitions.get(id).map(|t|t.enabled),"hooks":t.hooks})).collect::<Vec<_>>(),
        "operations":g.binding.operations.iter().enumerate().map(|(id,o)|json!({"id":id,"name":o.name,"kind":format!("{:?}",o.kind),"enabled":o.enabled!=0})).collect::<Vec<_>>()})
}
pub(super) fn inspect(world: &World, system: &str) -> Value {
    match system {
        "graphs" => world.get_resource::<StockGraphs>().map_or(
            Value::Null,
            |g| json!({"action":graph(&g.action),"motion":graph(&g.motion)}),
        ),
        "scoring" => world.resource::<SkaterRuntime>().scoring.mod_catalog(),
        _ => Value::Null,
    }
}

fn flag<'a>(
    g: &'a mut StockGraphs,
    graph: &str,
    target: &str,
    index: usize,
) -> Option<&'a mut bool> {
    let p = &mut if graph == "action" {
        &mut g.action
    } else {
        &mut g.motion
    }
    .runtime
    .program;
    match target {
        "state" => p.topology.states.get_mut(index).map(|s| &mut s.enabled),
        "transition" => p
            .topology
            .transitions
            .get_mut(index)
            .map(|t| &mut t.enabled),
        "behavior" => p.behaviors.get_mut(index).map(|b| &mut b.enabled),
        _ => None,
    }
}
pub(super) fn gate(
    world: &mut World,
    mods: &mut super::Mods,
    owner: &str,
    graph: String,
    target: String,
    index: usize,
    enabled: Option<bool>,
) -> Result<(), String> {
    let key = (graph, target, index);
    if mods.graph_gates.get(&key).is_some_and(|(o, _)| o != owner) {
        return Err("graph gate is owned by another mod".into());
    }
    let mut graphs = world
        .get_resource_mut::<StockGraphs>()
        .ok_or("graphs unavailable")?;
    let f = flag(&mut graphs, &key.0, &key.1, key.2).ok_or("unknown graph element")?;
    if let Some(enabled) = enabled {
        mods.graph_gates
            .entry(key)
            .or_insert((owner.to_owned(), *f));
        *f = enabled;
    } else if let Some((_, original)) = mods.graph_gates.remove(&key) {
        *f = original;
    }
    Ok(())
}
pub(super) fn restore_gates(world: &mut World, mods: &mut super::Mods, owner: Option<&str>) {
    let keys: Vec<_> = mods
        .graph_gates
        .iter()
        .filter(|(_, (o, _))| owner.is_none_or(|owner| o == owner))
        .map(|(k, _)| k.clone())
        .collect();
    for key in keys {
        if let Some((_, original)) = mods.graph_gates.remove(&key) {
            if let Some(mut graphs) = world.get_resource_mut::<StockGraphs>() {
                if let Some(f) = flag(&mut graphs, &key.0, &key.1, key.2) {
                    *f = original;
                }
            }
        }
    }
}
