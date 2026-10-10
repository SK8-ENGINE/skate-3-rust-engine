//! DMO streaming in the game (doc 27 "DMO streaming"): the map's placed props run through the retail DMO census
//! (`skate_core::living_world::dmo`, census slot 2 of 4) and a culled or evicted prop goes dormant
//! (`GamePhysics::stream_prop`: body asleep and still, collision parked, model hidden); a spawn brings it back at its
//! authored pose, as retail respawns a culled DMO from its placement record (research b98: nothing writes the live pose
//! into the record; the moved-pose map only rebuilds records at a chunk reload, not modelled). Props in the player's
//! saved placement layouts (#15, not retail) are never streamed, so layouts survive.
//!
//! On by default, as retail streams its DMOs; `SKATE_DMO_STREAM=0` (or a mod) turns it off, and then every placed
//! prop exists all the time. Logs `DMO_STREAM` (counts per change) and `DMO_STREAM_CHANGE` per decision.

use bevy::prelude::*;
use skate_core::living_world::dmo::{DmoDecision, DmoStream, DmoStreamSettings, DmoView, DmoWeights};
use skate_core::living_world::Observer;

/// The game's DMO streaming switch and values (retail defaults, the weights and range from the disc data).
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct DmoStreamGameSettings {
    pub enabled: bool,
    /// A streamed-in prop returns to its authored pose (retail); false keeps it where it was left.
    pub respawn_authored: bool,
    pub core: DmoStreamSettings,
}

impl Default for DmoStreamGameSettings {
    fn default() -> Self {
        Self { enabled: std::env::var("SKATE_DMO_STREAM").ok().as_deref() != Some("0"), respawn_authored: true, core: DmoStreamSettings::default() }
    }
}

/// The census state for the current map's props.
#[derive(Resource, Default)]
pub(crate) struct DmoStreamState {
    stream: Option<DmoStream>,
    /// The map generation and body count the stream was built for.
    built: Option<(u64, usize)>,
    /// The disc's range and weights (read once per map).
    data: Option<(skate_core::living_world::census::CensusRange, DmoWeights)>,
    tick: u64,
}

/// `livingworld_census_ranges` `dynamicobjects` and the `livingworld.dynamicobjects` weights from the setup export.
fn read_data(asset_root: &std::path::Path) -> Option<(skate_core::living_world::census::CensusRange, DmoWeights)> {
    let bytes = std::fs::read(asset_root.join("private/living_world/tables.json")).ok()?;
    let root: serde_json::Value = serde_json::from_slice(&bytes).ok()?;
    let circle = |name: &str| {
        let c = root.pointer(&format!("/classes/livingworld_census_ranges/dynamicobjects/fields/{name}"))?;
        let f = |k: &str| c.get(k).and_then(serde_json::Value::as_f64).map(|v| v as f32);
        Some(skate_core::living_world::census::CensusCircle {
            spawn_inner: f("spawn_inner")?,
            spawn_outer: f("spawn_outer")?,
            cull: f("cull")?,
            forward_offset: f("forward_offset")?,
            speed_kmh: f("speed_kmh")?,
        })
    };
    let range = skate_core::living_world::census::CensusRange { slow: circle("circle_slow")?, fast: circle("circle_fast")? };
    let w = root.pointer("/classes/livingworld/dynamicobjects/fields")?;
    let d = DmoWeights::default();
    let f = |k: &str, default: f32| w.get(k).and_then(serde_json::Value::as_f64).map_or(default, |v| v as f32);
    let weights = DmoWeights {
        in_front: f("Hash_74FE9111054663C9", d.in_front),
        keep_flag: f("Hash_883771F11981A45D", d.keep_flag),
        priority: f("Hash_16ADB1DBA4AA77D1", d.priority),
        count: f("Hash_C891B78CB013B593", d.count),
        distance: f("Hash_ABD7EA6B3B93846E", d.distance),
    };
    Some((range, weights))
}

/// One DMO census pass every 4th world tick (slot 2, `sub_826B71F0`); the first pass after a (re)build fills.
pub(crate) fn stream_dmos(
    settings: Res<super::LivingWorldSettings>,
    observers: Res<super::LivingWorldObservers>,
    config: Option<Res<crate::config::Config>>,
    map: Option<Res<crate::map_transition::CurrentMap>>,
    cameras: Query<&GlobalTransform, With<Camera3d>>,
    mut physics: Option<ResMut<crate::physics::GamePhysics>>,
    mut st: ResMut<DmoStreamState>,
) {
    let Some(physics) = physics.as_deref_mut() else { return };
    let s = settings.dmo_stream;
    let generation = map.as_ref().map_or(0, |m| m.generation);
    if !s.enabled || settings.net_role == super::NetRole::Client {
        // Off (or a mod turned it off): everything streams back in.
        if let Some(stream) = st.stream.take() {
            for p in &stream.placements {
                physics.stream_prop(p.id, false, false);
            }
            info!("DMO_STREAM off: {} props back", stream.placements.len());
        }
        st.built = None;
        return;
    }
    let Some(dynamics) = physics.prop_dynamics() else { return };
    let placements = dynamics.dmo_placements();
    let fill = st.built != Some((generation, placements.len()));
    if fill {
        if st.data.is_none() || st.built.is_none_or(|b| b.0 != generation) {
            st.data = config.as_ref().and_then(|c| read_data(&c.asset_root));
        }
        // Map load: the census starts empty and the first (fill) pass picks the live set; every other prop goes
        // dormant after it (retail's initial populate spawns from nothing).
        st.stream = Some(DmoStream::new(placements, &s.core));
        st.built = Some((generation, st.stream.as_ref().map_or(0, |s| s.placements.len())));
        st.tick = 0;
    }
    st.tick += 1;
    if !fill && st.tick % skate_core::living_world::config::retail::CENSUS_ROTATION as u64 != 2 {
        return;
    }
    let mut core = s.core;
    if let Some((range, weights)) = st.data {
        core.range = range;
        core.weights = weights;
    }
    let camera = cameras.iter().next().map(|t| (t.translation().to_array(), t.forward().as_vec3().to_array()));
    let views: Vec<(Observer, DmoView)> = observers
        .observers
        .iter()
        .map(|o| {
            let (c, f) = camera.unwrap_or((o.position, [0.0, 0.0, 1.0]));
            (*o, DmoView { camera: c, forward: f, reference: o.position })
        })
        .collect();
    let held = physics.prop_carry.held();
    let layout: std::collections::BTreeSet<u32> = physics.prop_carry.layout().keys().copied().collect();
    let Some(stream) = st.stream.as_mut() else { return };
    let decisions = stream.step(&views, &core, fill, &|id| held == Some(id) || layout.contains(&id));
    if decisions.is_empty() && !fill {
        return;
    }
    let (mut spawned, mut culled, mut evicted) = (0, 0, 0);
    for d in &decisions {
        let (id, out) = match *d {
            DmoDecision::Spawn(id) => {
                spawned += 1;
                (id, false)
            }
            DmoDecision::Cull(id) => {
                culled += 1;
                (id, true)
            }
            DmoDecision::Evict(id) => {
                evicted += 1;
                (id, true)
            }
        };
        if !fill {
            debug!("DMO_STREAM_CHANGE {d:?}");
        }
        physics.stream_prop(id, out, s.respawn_authored);
    }
    if fill {
        let dormant: Vec<u32> = st.stream.as_ref().map_or_else(Vec::new, |s| s.placements.iter().filter(|p| !s.live.contains(&p.id) && held != Some(p.id) && !layout.contains(&p.id)).map(|p| p.id).collect());
        culled += dormant.len();
        for id in dormant {
            physics.stream_prop(id, true, false);
        }
    }
    let live = st.stream.as_ref().map_or(0, |s| s.live.len());
    info!("DMO_STREAM fill={fill} spawned={spawned} culled={culled} evicted={evicted} live={live}");
}
