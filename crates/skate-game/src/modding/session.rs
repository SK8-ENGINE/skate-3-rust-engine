//! Generic session authority. The transport host ratifies control transfers;
//! relocations are accepted only from that authority in the current generation.
use super::{observation, Mods};
use bevy::prelude::*;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use skate_core::physics::skeleton_animation_record::IDENTITY;
use skate_mods::TeleportOptions;
use std::collections::BTreeMap;

pub const AUTH_KEY: &str = "s2:auth";
const REQUEST_KEY: &str = "s2:authority_request";
const TELEPORT_PREFIX: &str = "s2:t:";

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct AuthWire { g: u64, a: String }

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct TeleportWire {
    g: u64, s: u32, p: [f32; 3],
    #[serde(default)] h: Option<f32>,
    #[serde(default)] v: Option<[f32; 3]>,
}

#[derive(Default)]
pub(super) struct Runtime {
    connection: Option<(u64, u64, u64)>,
    generation: u64,
    advertised: Option<String>,
    applied: BTreeMap<u64, u32>,
    teleport_seq: u32,
    pending_auth: Option<AuthWire>,
    pending_teleports: BTreeMap<String, TeleportWire>,
}
impl Runtime {
    pub fn reset(&mut self) { *self = Self::default(); }
    pub fn connect(&mut self, connection: Option<(u64, u64, u64)>) {
        if self.connection != connection { self.reset(); self.connection = connection; }
    }
    fn accept(&mut self, auth: AuthWire) {
        if auth.g > self.generation {
            self.applied.clear();
            self.pending_teleports.clear();
        }
        self.generation = auth.g;
        self.advertised = Some(auth.a);
        if self.pending_auth.as_ref().is_some_and(|p| p.g <= self.generation) {
            self.pending_auth = None;
        }
    }
}
pub(super) fn spawn_matrix(position: [f32; 3], heading: Option<f32>) -> [[f32; 4]; 4] {
    let mut result = IDENTITY;
    let h = heading.unwrap_or(0.);
    result[2] = [h.sin(), 0., h.cos(), 0.];
    result[0] = [h.cos(), 0., -h.sin(), 0.];
    result[3] = [position[0], position[1], position[2], 0.];
    result
}
pub(super) fn net_identity(world: &World) -> (bool, String, bool, String) {
    match world.get_resource::<crate::multiplayer::Multiplayer>() {
        Some(net) if net.active() => {
            let (active, id, host) = net.mod_identity();
            (active, id.to_string(), host, net.host_actor().to_string())
        }
        _ => (false, "0".into(), true, "0".into()),
    }
}
fn connected(world: &World, peer: &str) -> bool {
    let (active, local, _, _) = net_identity(world);
    if !active { return peer == local; }
    world.resource::<crate::multiplayer::Multiplayer>().player_ids().iter().any(|id| id.to_string() == peer)
}
pub(super) fn apply_local(world: &mut World, options: &TeleportOptions) -> Result<(), String> {
    if !options.validate() { return Err("invalid teleport options".into()); }
    world.resource_mut::<crate::physics::SkaterRuntime>().travel(spawn_matrix(options.position, options.heading), options.velocity)
}
pub(super) fn resolved_authority(mods: &Mods, local: &str, host: bool, host_peer: &str) -> String {
    mods.session.advertised.clone().unwrap_or_else(|| if host { local.into() } else { host_peer.into() })
}
pub(super) fn claim(world: &World, mods: &mut Mods) -> Result<(), String> {
    let (_, local, host, host_peer) = net_identity(world);
    if !host && resolved_authority(mods, &local, host, &host_peer) != local {
        return Err("session authority is held by another player".into());
    }
    request(mods, local, host);
    Ok(())
}
fn request(mods: &mut Mods, peer: String, host: bool) {
    let auth = AuthWire { g: mods.session.generation.saturating_add(1), a: peer };
    if host { mods.session.accept(auth); } else { mods.session.pending_auth = Some(auth); }
}
pub(super) fn transfer(world: &World, mods: &mut Mods, peer: &str) -> Result<(), String> {
    let (_, local, host, host_peer) = net_identity(world);
    if resolved_authority(mods, &local, host, &host_peer) != local { return Err("only session authority can transfer control".into()); }
    if !connected(world, peer) { return Err("target player is not connected".into()); }
    request(mods, peer.into(), host);
    Ok(())
}
pub(super) fn teleport(world: &mut World, mods: &mut Mods, peer: &str, options: TeleportOptions) -> Result<(), String> {
    let (_, local, host, host_peer) = net_identity(world);
    if resolved_authority(mods, &local, host, &host_peer) != local { return Err("only session authority can relocate other players".into()); }
    if !connected(world, peer) { return Err("target player is not connected".into()); }
    if !options.validate() { return Err("invalid teleport options".into()); }
    if peer == local { return apply_local(world, &options); }
    mods.session.teleport_seq = mods.session.teleport_seq.saturating_add(1);
    mods.session.pending_teleports.insert(peer.into(), TeleportWire {
        g: mods.session.generation, s: mods.session.teleport_seq, p: options.position, h: options.heading, v: options.velocity,
    });
    Ok(())
}

fn receive_authority(state: &mut Runtime, records: &[(u64, String, u32, Vec<u8>)], local: u64, host: u64, players: &[u64]) {
    if local != host {
        for (peer, key, _, bytes) in records {
            if *peer != host || key != AUTH_KEY { continue; }
            if let Ok(auth) = serde_json::from_slice::<AuthWire>(bytes) {
                if auth.g >= state.generation && auth.a.parse::<u64>().is_ok_and(|id| players.contains(&id)) { state.accept(auth); }
            }
        }
    } else {
        let authority = state.advertised.as_deref().and_then(|a| a.parse::<u64>().ok()).unwrap_or(host);
        if !players.contains(&authority) {
            state.accept(AuthWire { g: state.generation.saturating_add(1), a: host.to_string() });
        }
        for (peer, key, _, bytes) in records {
            let current = state.advertised.as_deref().and_then(|a| a.parse::<u64>().ok()).unwrap_or(host);
            if *peer != current || key != REQUEST_KEY { continue; }
            if let Ok(auth) = serde_json::from_slice::<AuthWire>(bytes) {
                if auth.g == state.generation.saturating_add(1) && auth.a.parse::<u64>().is_ok_and(|id| players.contains(&id)) { state.accept(auth); }
            }
        }
    }
}
fn receive_teleport(state: &mut Runtime, peer: u64, bytes: &[u8], authority: u64) -> Option<TeleportOptions> {
    if peer != authority { return None; }
    let cmd: TeleportWire = serde_json::from_slice(bytes).ok()?;
    if cmd.g != state.generation || cmd.s <= state.applied.get(&peer).copied().unwrap_or(0) { return None; }
    let options = TeleportOptions { position: cmd.p, heading: cmd.h, velocity: cmd.v };
    if !options.validate() { return None; }
    state.applied.insert(peer, cmd.s);
    Some(options)
}
pub(super) fn ingest(mods: &mut Mods, records: &[(u64, String, u32, Vec<u8>)], local: u64, host: u64, players: &[u64]) {
    mods.skater_remote.retain(|peer, _| players.contains(peer) && records.iter().any(|(p,k,_,b)| p == peer && k == observation::OBS_KEY && !b.is_empty()));
    receive_authority(&mut mods.session, records, local, host, players);
    let authority = resolved_authority(mods, &local.to_string(), local == host, &host.to_string()).parse::<u64>().unwrap_or(host);
    for (peer, key, _, bytes) in records {
        if !players.contains(peer) { continue; }
        if key == observation::OBS_KEY {
            if let Some(obs) = observation::decode(bytes) { mods.skater_remote.insert(*peer, obs); }
        } else if key.strip_prefix(TELEPORT_PREFIX) == Some(local.to_string().as_str()) {
            if let Some(options) = receive_teleport(&mut mods.session, *peer, bytes, authority) { mods.pending_remote_teleport = Some(options); }
        }
    }
}
pub(super) fn apply_pending_remote(world: &mut World, mods: &mut Mods) {
    if let Some(options) = mods.pending_remote_teleport.take() {
        if let Err(error) = apply_local(world, &options) { warn!("session teleport: {error}"); }
    }
}
pub(super) fn outgoing(mods: &Mods, local: &str, host: bool, host_peer: &str) -> BTreeMap<String, Vec<u8>> {
    let mut out = BTreeMap::new();
    let authority = resolved_authority(mods, local, host, host_peer);
    if host {
        let auth = AuthWire { g: mods.session.generation, a: authority.clone() };
        out.insert(AUTH_KEY.into(), serde_json::to_vec(&auth).unwrap());
    } else if let Some(request) = &mods.session.pending_auth {
        out.insert(REQUEST_KEY.into(), serde_json::to_vec(request).unwrap());
    }
    if authority == local {
        for (peer, cmd) in &mods.session.pending_teleports { out.insert(format!("{TELEPORT_PREFIX}{peer}"), serde_json::to_vec(cmd).unwrap()); }
    }
    out
}
pub(super) fn lua(mods: &Mods, active: bool, local: &str, host: bool, host_peer: &str, players: &[String]) -> Value {
    json!({"active":active,"local_id":local,"is_host":host,"authority":resolved_authority(mods,local,host,host_peer),"players":players})
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn transfers_require_host_ratification_and_current_authority() {
        let mut state = Runtime::default();
        let forged = (3, AUTH_KEY.into(), 1, serde_json::to_vec(&AuthWire {g:99,a:"3".into()}).unwrap());
        receive_authority(&mut state, &[forged], 2, 1, &[1,2,3]);
        assert!(state.advertised.is_none());
        let host = (1, AUTH_KEY.into(), 1, serde_json::to_vec(&AuthWire {g:1,a:"2".into()}).unwrap());
        receive_authority(&mut state, &[host], 2, 1, &[1,2,3]);
        assert_eq!(state.advertised.as_deref(), Some("2"));
        let request = |peer| (peer, REQUEST_KEY.into(), 1, serde_json::to_vec(&AuthWire {g:2,a:"3".into()}).unwrap());
        receive_authority(&mut state, &[request(3)], 1, 1, &[1,2,3]);
        assert_eq!(state.generation, 1);
        receive_authority(&mut state, &[request(2)], 1, 1, &[1,2,3]);
        assert_eq!(state.advertised.as_deref(), Some("3"));
        receive_authority(&mut state, &[], 1, 1, &[1,2]);
        assert_eq!(state.advertised.as_deref(), Some("1"));
    }
    #[test]
    fn teleports_reject_wrong_sender_invalid_values_replays_and_old_generations() {
        let mut state = Runtime::default();
        let packet = |g, s, x| serde_json::to_vec(&TeleportWire {g,s,p:[x,0.,0.],h:None,v:None}).unwrap();
        assert!(receive_teleport(&mut state, 3, &packet(0,1,1.), 1).is_none());
        assert!(receive_teleport(&mut state, 1, &packet(0,1,200_000.), 1).is_none());
        assert!(receive_teleport(&mut state, 1, &packet(0,1,1.), 1).is_some());
        assert!(receive_teleport(&mut state, 1, &packet(0,1,1.), 1).is_none());
        state.accept(AuthWire {g:1,a:"1".into()});
        assert!(receive_teleport(&mut state, 1, &packet(0,2,1.), 1).is_none());
        assert!(receive_teleport(&mut state, 1, &packet(1,1,1.), 1).is_some());
    }
}
