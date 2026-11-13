//! Living-world events for mods: `on_event {name = "living_world", event = ...}` (engine modding, no retail).
//! Event list and fields: docs/hails-additions/26a-living-world-overview.md#living-world-mod-events

use super::Mods;
use crate::living_world::npc_skaters::{trick_name, NpcSkaterEvent};
use crate::living_world::peds::PedEvent;
use crate::living_world::vehicle_contacts::VehicleContactEvent;
use crate::living_world::{LivingWorldDespawn, LivingWorldSpawn, WireRecord};
use bevy::prelude::*;
use serde_json::{json, Value};
use skate_core::living_world::Decision;

pub(super) fn install(app: &mut App) {
    app.add_systems(Update, forward.after(crate::app::FrameSet::Animation).before(super::update));
}

fn hex(id: &[u8; 16]) -> String {
    id.iter().map(|b| format!("{b:02x}")).collect()
}

/// The event payloads for this frame's messages (also used by the tests).
pub(crate) fn payloads(
    spawns: impl IntoIterator<Item = LivingWorldSpawn>,
    despawns: impl IntoIterator<Item = LivingWorldDespawn>,
    npc: impl IntoIterator<Item = NpcSkaterEvent>,
    contacts: impl IntoIterator<Item = VehicleContactEvent>,
    peds: impl IntoIterator<Item = PedEvent>,
) -> Vec<Value> {
    let mut out = Vec::new();
    let record = |d: Decision| serde_json::to_value(WireRecord::from_decision(&d)).unwrap_or(Value::Null);
    for s in spawns {
        out.push(json!({"name": "living_world", "event": "spawn", "record": record(Decision::Spawn(s.0))}));
    }
    for d in despawns {
        out.push(json!({"name": "living_world", "event": "despawn", "record": record(Decision::Despawn(d.0))}));
    }
    for e in npc {
        match e {
            NpcSkaterEvent::Trick { id, record } => out.push(json!({
                "name": "living_world", "event": "npc_trick", "id": id.to_u64(), "line": hex(&record.line), "node": record.node,
                "recorded": trick_name(record.recorded), "chosen": trick_name(record.chosen),
            })),
            NpcSkaterEvent::LineEnd { id } => out.push(json!({"name": "living_world", "event": "npc_line_end", "id": id.to_u64()})),
            NpcSkaterEvent::Bail { id, respawn_seconds } => out.push(json!({"name": "living_world", "event": "npc_bail", "id": id.to_u64(), "respawn_seconds": respawn_seconds})),
            NpcSkaterEvent::Respawned { id, node } => out.push(json!({"name": "living_world", "event": "npc_respawn", "id": id.to_u64(), "node": node})),
            NpcSkaterEvent::WalkBack { id, node, started } => out.push(json!({"name": "living_world", "event": "npc_walk_back", "id": id.to_u64(), "node": node, "started": started})),
            NpcSkaterEvent::Avoid { id, mode, target } => out.push(json!({
                "name": "living_world", "event": "npc_avoid", "id": id.to_u64(), "mode": mode.name(),
                "target_kind": target.map(|t| format!("{:?}", t.0).to_lowercase()), "target_id": target.map(|t| t.1),
            })),
            _ => {}
        }
    }
    for p in peds {
        match p {
            PedEvent::Hit { id, kind, direction, closing, toucher } => {
                out.push(json!({"name": "living_world", "event": "ped_hit", "id": id.to_u64(), "kind": kind.name(), "direction": direction.name(), "closing": closing, "toucher": toucher}))
            }
            // A stored reaction played at the ped's next Colliding (a contact during its takedown attempt, or a failed
            // takedown's trip with no toucher): the same event, marked deferred.
            PedEvent::DeferredHit { id, kind, direction, closing, toucher } => {
                out.push(json!({"name": "living_world", "event": "ped_hit", "id": id.to_u64(), "kind": kind.name(), "direction": direction.name(), "closing": closing, "toucher": toucher, "deferred": true}))
            }
            PedEvent::Chase { id, chasee, kind, reason } => out.push(json!({"name": "living_world", "event": "ped_chase", "id": id.to_u64(), "chasee": chasee, "kind": kind, "reason": reason})),
            PedEvent::Avoid { id, mover, kind, dir, seated } => out.push(json!({"name": "living_world", "event": "ped_avoid", "id": id.to_u64(), "mover": mover, "kind": kind, "dir": dir, "seated": seated})),
            PedEvent::Taze { id, target } => out.push(json!({"name": "living_world", "event": "ped_taze", "id": id.to_u64(), "target": target})),
            PedEvent::Takedown { id, target, success } => out.push(json!({"name": "living_world", "event": "ped_takedown", "id": id.to_u64(), "target": target, "success": success})),
            PedEvent::Speech { id, value, topic, state } => out.push(json!({"name": "living_world", "event": "ped_speech", "id": id.to_u64(), "value": value, "variant": topic.map(|t| t.0), "list_value": topic.map(|t| t.1), "state": state})),
            PedEvent::HandProp { id, key } => out.push(json!({"name": "living_world", "event": "ped_hand_prop", "id": id.to_u64(), "hand_prop": key})),
            _ => {}
        }
    }
    for c in contacts {
        out.push(json!({"name": "living_world", "event": "vehicle_contact", "contact": serde_json::to_value(&c).unwrap_or(Value::Null)}));
    }
    out
}

fn forward(
    mods: Option<ResMut<Mods>>,
    mut spawns: MessageReader<LivingWorldSpawn>,
    mut despawns: MessageReader<LivingWorldDespawn>,
    mut npc: MessageReader<NpcSkaterEvent>,
    mut contacts: MessageReader<VehicleContactEvent>,
    mut peds: MessageReader<PedEvent>,
) {
    let Some(mut mods) = mods else {
        spawns.clear();
        despawns.clear();
        npc.clear();
        contacts.clear();
        peds.clear();
        return;
    };
    for payload in payloads(spawns.read().cloned(), despawns.read().cloned(), npc.read().cloned(), contacts.read().cloned(), peds.read().cloned()) {
        mods.manager.dispatch("on_event", payload);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use skate_core::living_world::{DespawnReason, DespawnRecord, Kind, LivingWorldId};

    #[test]
    fn living_world_messages_become_mod_events() {
        let id = LivingWorldId { kind: Kind::Pedestrian, serial: 7 };
        let out = payloads(
            [],
            [LivingWorldDespawn(DespawnRecord { id, tick: 12, reason: DespawnReason::External })],
            [NpcSkaterEvent::Trick {
                id: LivingWorldId { kind: Kind::Skater, serial: 2 },
                record: skate_core::living_world::replay::TrickRecord { frame: 3, line: [0xab; 16], node: 9, recorded: 128, chosen: 96 },
            }],
            [],
            [PedEvent::Hit {
                id: LivingWorldId { kind: Kind::Pedestrian, serial: 4 },
                kind: skate_core::living_world::peds::skater_contact::ReactionKind::Knockdown,
                direction: skate_core::living_world::peds::skater_contact::ReactionDirection::FromBack,
                closing: 6.5,
                toucher: 9,
            }, PedEvent::Speech { id: LivingWorldId { kind: Kind::Pedestrian, serial: 4 }, value: 53, topic: None, state: "Warn".into() }, PedEvent::Chase { id: LivingWorldId { kind: Kind::Pedestrian, serial: 4 }, chasee: 9, kind: "group_end", reason: Some(1) }],
        );
        assert_eq!(out.len(), 5);
        assert_eq!((out[4]["event"].as_str(), out[4]["kind"].as_str(), out[4]["reason"].as_i64()), (Some("ped_chase"), Some("group_end"), Some(1)));
        assert_eq!((out[3]["event"].as_str(), out[3]["value"].as_i64(), out[3]["state"].as_str()), (Some("ped_speech"), Some(53), Some("Warn")));
        assert_eq!((out[2]["event"].as_str(), out[2]["kind"].as_str(), out[2]["direction"].as_str()), (Some("ped_hit"), Some("Knockdown"), Some("FromBack")));
        assert_eq!(out[0]["event"], "despawn");
        assert_eq!(out[0]["name"], "living_world");
        assert!(out[0]["record"].is_object(), "{}", out[0]);
        assert_eq!(out[1]["event"], "npc_trick");
        assert_eq!((out[1]["recorded"].as_str(), out[1]["chosen"].as_str()), (Some("ollie"), Some("kickflip")));
        assert_eq!(out[1]["node"], 9);
    }
}
