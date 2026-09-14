//! Compact skater observation for the SDK. Each peer publishes its local skater;
//! remotes are assembled from APPLICATION `s2:obs` plus native body poses.
use super::Mods;
use bevy::prelude::*;
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use skate_core::physics::board::BodyId;
use std::collections::BTreeMap;

pub const OBS_KEY: &str = "s2:obs";
const MAX_INTENTS: usize = 16;
const MAX_INTENT_NAME: usize = 32;
const MAX_TRICK: usize = 64;
const MAX_MODE: usize = 16;
const MAX_GRIND: usize = 16;

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct WireObs {
    #[serde(default)]
    pub p: [f32; 3],
    #[serde(default)]
    pub v: [f32; 3],
    #[serde(default)]
    pub av: [f32; 3],
    #[serde(default)]
    pub fw: [f32; 3],
    #[serde(default)]
    pub h: f32,
    #[serde(default)]
    pub r: [f32; 4],
    #[serde(default)]
    pub sp: f32,
    #[serde(default)]
    pub ob: bool,
    #[serde(default)]
    pub st: u32,
    #[serde(default)]
    pub ca: u32,
    #[serde(default)]
    pub fl: u32,
    #[serde(default)]
    pub ba: bool,
    #[serde(default)]
    pub tr: String,
    #[serde(default)]
    pub ts: u32,
    #[serde(default)]
    pub landed: u32,
    #[serde(default)]
    pub landed_name: String,
    #[serde(default)]
    pub bail_seq: u32,
    #[serde(default)]
    pub nt: bool,
    #[serde(default)]
    pub mt: bool,
    #[serde(default)]
    pub ct: bool,
    #[serde(default)]
    pub sa: bool,
    #[serde(default)]
    pub sc: f32,
    #[serde(default)]
    pub ls: f32,
    #[serde(default)]
    pub mu: f32,
    #[serde(default)]
    pub lt: f32,
    #[serde(default)]
    pub cl: bool,
    #[serde(default)]
    pub sk: bool,
    #[serde(default)]
    pub sw: bool,
    #[serde(default)]
    pub fk: bool,
    #[serde(default)]
    pub nl: bool,
    #[serde(default)]
    pub md: String,
    #[serde(default)]
    pub gd: String,
    #[serde(default, rename = "in")]
    pub inn: BTreeMap<String, f32>,
}

impl WireObs {
    fn valid(&self) -> bool {
        self.p.iter().all(|v| v.is_finite() && v.abs() <= 100_000.)
            && self.v.iter().all(|v| v.is_finite() && v.abs() <= 200.)
            && self.av.iter().all(|v| v.is_finite() && v.abs() <= 10_000.)
            && self.fw.iter().all(|v| v.is_finite())
            && self.h.is_finite()
            && self.r.iter().all(|v| v.is_finite())
            && self.sp.is_finite()
            && self.sc.is_finite()
            && self.ls.is_finite()
            && self.mu.is_finite()
            && self.lt.is_finite()
            && self.landed_name.len() <= MAX_TRICK
            && self.tr.len() <= MAX_TRICK
            && self.md.len() <= MAX_MODE
            && self.gd.len() <= MAX_GRIND
            && self.inn.len() <= MAX_INTENTS
            && self
                .inn
                .iter()
                .all(|(n, v)| n.len() <= MAX_INTENT_NAME && v.is_finite())
    }
}

pub(super) fn encode_local(world: &World) -> Option<Vec<u8>> {
    let obs = local(world);
    let mut bytes = serde_json::to_vec(&obs).ok()?;
    if bytes.len() <= skate_net::lobby::MAX_APP_VALUE {
        return Some(bytes);
    }
    let mut slim = obs;
    while bytes.len() > skate_net::lobby::MAX_APP_VALUE && !slim.inn.is_empty() {
        if let Some(key) = slim
            .inn
            .iter()
            .min_by(|a, b| a.1.abs().total_cmp(&b.1.abs()))
            .map(|(k, _)| k.clone())
        {
            slim.inn.remove(&key);
        } else {
            break;
        }
        bytes = serde_json::to_vec(&slim).ok()?;
    }
    (bytes.len() <= skate_net::lobby::MAX_APP_VALUE).then_some(bytes)
}

pub(super) fn decode(bytes: &[u8]) -> Option<WireObs> {
    let obs: WireObs = serde_json::from_slice(bytes).ok()?;
    obs.valid().then_some(obs)
}

pub(super) fn local(world: &World) -> WireObs {
    let s = world.resource::<crate::physics::SkaterRuntime>();
    let p = &s.player_input.physical;
    let physics = world.resource::<crate::physics::GamePhysics>();
    let controls = world.resource::<crate::physics::PlayerControls>();
    let root = s.animated_skeleton.roots.animation_to_world;
    let heading = root[2][0].atan2(root[2][2]);
    let rotation = quat_from_basis(root);
    let mut inn = BTreeMap::new();
    for (name, value) in &controls.named_intents {
        if inn.len() >= MAX_INTENTS {
            break;
        }
        if name.len() <= MAX_INTENT_NAME && value.abs() > 0.01 {
            inn.insert(name.clone(), *value);
        }
    }
    let vel = {
        let v = p.skateboard.vector_80.map(f32::from_bits);
        [v[0], v[1], v[2]]
    };
    let deck = physics.board.bodies()[BodyId::Deck.index()];
    let av = [
        deck.rates.angular_velocity.x,
        deck.rates.angular_velocity.y,
        deck.rates.angular_velocity.z,
    ];
    let forward = [root[2][0], root[2][1], root[2][2]];
    let stance = s.scoring.stance();
    let grind = s.grind.active_name().unwrap_or("").to_owned();
    let cat = p.state.category_12;
    let bail = physics.board_wiping_out;
    let mode = mode_name(cat, p.filtered_state_0, !grind.is_empty(), bail).to_owned();
    let trick = crate::scoring_hud::display_trick(world, s.scoring.trick_name());
    WireObs {
        p: [root[3][0], root[3][1], root[3][2]],
        v: vel,
        av,
        fw: forward,
        h: heading,
        r: rotation,
        sp: (vel[0] * vel[0] + vel[1] * vel[1] + vel[2] * vel[2]).sqrt(),
        ob: cat != 500,
        st: p.state.state_16,
        ca: cat,
        fl: p.filtered_state_0,
        ba: bail,
        tr: bounded_label(&trick, MAX_TRICK),
        ts: s.scoring.trick_seq(),
        landed: s.scoring.landing_seq,
        landed_name: bounded_label(&crate::scoring_hud::display_trick(world, &s.scoring.landed_trick), MAX_TRICK),
        bail_seq: s.scoring.bail_seq,
        nt: s.scoring.new_trick,
        mt: s.scoring.modified_trick,
        ct: s.scoring.close_tricks,
        sa: s.scoring.sequence_active(),
        sc: s.scoring.sequence_score(),
        ls: s.scoring.line_score(),
        mu: s.scoring.multiplier(),
        lt: s.scoring.line_time(),
        cl: s.scoring.clean(),
        sk: s.scoring.sketchy(),
        sw: stance[0],
        fk: stance[1],
        nl: stance[3],
        md: mode,
        gd: bounded_label(&grind, MAX_GRIND),
        inn,
    }
}

fn bounded_label(text: &str, max: usize) -> String {
    let mut end = text.len().min(max);
    while !text.is_char_boundary(end) { end -= 1; }
    text[..end].to_owned()
}

fn mode_name(category: u32, filtered: u32, grinding: bool, bail: bool) -> &'static str {
    if bail {
        return "bail";
    }
    if grinding || (400..=405).contains(&category) {
        return "grind";
    }
    match filtered {
        1 => "ground",
        2 => "air",
        3 => "grind",
        4 => "bail",
        5 => "teleport",
        6 => "offboard",
        7 => "offboard_air",
        _ if category == 500 => "offboard",
        _ if category == 200 => "air",
        _ => "ground",
    }
}

fn lua_fields(obs: &WireObs) -> Map<String, Value> {
    let mut out = Map::new();
    let insert = |out: &mut Map<String, Value>, key: &str, value: Value| {
        out.insert(key.to_owned(), value);
    };
    insert(&mut out, "position", json!(obs.p));
    insert(&mut out, "velocity", json!(obs.v));
    insert(&mut out, "angvel", json!(obs.av));
    insert(&mut out, "forward", json!(obs.fw));
    insert(&mut out, "heading", json!(obs.h));
    insert(&mut out, "rotation", json!(obs.r));
    insert(&mut out, "speed", json!(obs.sp));
    insert(&mut out, "on_board", json!(obs.ob));
    insert(&mut out, "state", json!(obs.st));
    insert(&mut out, "category", json!(obs.ca));
    insert(&mut out, "filtered", json!(obs.fl));
    insert(&mut out, "mode", json!(obs.md));
    insert(&mut out, "grind", json!(if obs.gd.is_empty() { Value::Null } else { json!(obs.gd) }));
    insert(&mut out, "bailing", json!(obs.ba));
    insert(&mut out, "trick", json!(obs.tr));
    insert(&mut out, "trick_seq", json!(obs.ts));
    insert(&mut out, "landing_seq", json!(obs.landed));
    insert(&mut out, "landed_trick", json!(obs.landed_name));
    insert(&mut out, "bail_seq", json!(obs.bail_seq));
    insert(&mut out, "new_trick", json!(obs.nt));
    insert(&mut out, "modified_trick", json!(obs.mt));
    insert(&mut out, "close_tricks", json!(obs.ct));
    insert(&mut out, "sequence", json!(obs.sa));
    insert(&mut out, "score", json!(obs.sc));
    insert(&mut out, "line", json!(obs.ls));
    insert(&mut out, "multiplier", json!(obs.mu));
    insert(&mut out, "line_time", json!(obs.lt));
    insert(&mut out, "clean", json!(obs.cl));
    insert(&mut out, "sketchy", json!(obs.sk));
    insert(&mut out, "switch", json!(obs.sw));
    insert(&mut out, "fakie", json!(obs.fk));
    insert(&mut out, "nollie", json!(obs.nl));
    insert(&mut out, "intents", json!(obs.inn));
    out
}

pub(super) fn lua_skater(id: &str, local: bool, obs: &WireObs, name: &str) -> Value {
    let mut fields = lua_fields(obs);
    fields.insert("id".into(), json!(id));
    fields.insert("local".into(), json!(local));
    fields.insert("name".into(), json!(name));
    Value::Object(fields)
}

pub(super) fn lua_player(obs: &WireObs, name: &str) -> Value {
    let mut fields = lua_fields(obs);
    fields.insert("name".into(), json!(name));
    Value::Object(fields)
}

fn name_for(world: &World, id: &str, local_id: &str) -> String {
    let Some(net) = world.get_resource::<crate::multiplayer::Multiplayer>() else {
        return id.to_owned();
    };
    net.skater_name(id, local_id)
}

pub(super) fn snapshot(world: &World, mods: &Mods, local_id: &str) -> (Value, Value) {
    let local_obs = local(world);
    let local_name = name_for(world, local_id, local_id);
    let mut skaters = Map::new();
    skaters.insert(
        local_id.to_owned(),
        lua_skater(local_id, true, &local_obs, &local_name),
    );
    for (peer, obs) in &mods.skater_remote {
        let id = peer.to_string();
        if id == local_id {
            continue;
        }
        let name = name_for(world, &id, local_id);
        skaters.insert(id.clone(), lua_skater(&id, false, obs, &name));
    }
    (lua_player(&local_obs, &local_name), Value::Object(skaters))
}

fn quat_from_basis(m: [[f32; 4]; 4]) -> [f32; 4] {
    let rot = Mat3::from_cols(
        Vec3::new(m[0][0], m[0][1], m[0][2]),
        Vec3::new(m[1][0], m[1][1], m[1][2]),
        Vec3::new(m[2][0], m[2][1], m[2][2]),
    );
    let q = Quat::from_mat3(&rot).normalize();
    q.to_array()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wire_obs_fits_application_budget() {
        let mut inn = BTreeMap::new();
        for i in 0..MAX_INTENTS {
            inn.insert(format!("IntentName{i:02}"), 1.0);
        }
        let obs = WireObs {
            p: [12.5, 3.0, -40.25],
            v: [4.0, 0.2, -1.5],
            av: [0.1, 1.2, -0.3],
            fw: [0.0, 0.0, 1.0],
            h: 1.2,
            r: [0.0, 0.1, 0.0, 0.995],
            sp: 4.3,
            ob: true,
            st: 100,
            ca: 100,
            fl: 1,
            ba: false,
            tr: "360Flip".into(),
            ts: 12,
            landed: 3,
            landed_name: "360Flip".into(),
            bail_seq: 1,
            nt: true,
            mt: false,
            ct: false,
            sa: true,
            sc: 240.0,
            ls: 80.0,
            mu: 1.5,
            lt: 12.0,
            cl: true,
            sk: false,
            sw: false,
            fk: false,
            nl: true,
            md: "air".into(),
            gd: String::new(),
            inn,
        };
        let bytes = serde_json::to_vec(&obs).unwrap();
        assert!(bytes.len() <= skate_net::lobby::MAX_APP_VALUE, "{}", bytes.len());
        assert!(decode(&bytes).is_some());
        let player = lua_player(&obs, "Test");
        assert_eq!(player["trick"], "360Flip");
        assert_eq!(player["trick_seq"], 12);
        assert_eq!(player["nollie"], true);
        assert_eq!(player["mode"], "air");
    }
}
