//! `MANUAL_LANDING` diagnostic line, one per landing (logs must diagnose: landings that "should
//! have" gone into a manual and did not). Engine logging only, no gameplay effect.
//!
//! - **Asked for:** the Manual intent (the stick's forward / back value) on the landing tick, how
//!   long it was held before touching down, and the motion graph's `ManualEngageTime` intent at
//!   touchdown (retail's motion graph only takes the Manual transition with `HasIntent
//!   ManualEngageTime > 0.2`, from InAirStatic / BoardAdjusts).
//! - **Where the graph was:** the motion graph state (id and authored name) and the playing clip
//!   at touchdown, so a landing from a trick or grab state (no Manual transition) is visible.
//! - **Granted:** the manual (balance) state flag (`+340`) comes on within [`WINDOW`] after the
//!   landing. "Why not" names what the log can see: no stick, the stick let go before the wheels
//!   came down, or the graph declined it with the stick held.
use super::{controls::PlayerControls, skater::SkaterRuntime, GamePhysics};
use crate::graph_runtime::StockGraphs;
use bevy::prelude::*;

/// How long after touchdown a manual still counts as the landing's (s).
const WINDOW: f32 = 0.5;

#[derive(Default)]
pub(crate) struct Seen {
    was_air: bool,
    /// Manual intent held continuously (s).
    held: f32,
    pending: Option<Pending>,
}

struct Pending {
    tick: u64,
    stick: [f32; 2],
    intent: Option<f32>,
    engage: Option<f32>,
    held: f32,
    wheels: usize,
    since: f32,
    intent_lost: bool,
    graph_state: String,
    clip: String,
}

pub(crate) fn log_manual_landings(
    physics: Res<GamePhysics>,
    skater: Res<SkaterRuntime>,
    controls: Res<PlayerControls>,
    graphs: Option<Res<StockGraphs>>,
    time: Res<Time>,
    mut seen: Local<Seen>,
) {
    let dt = time.delta_secs();
    let state = skater.player_state.current() as u32;
    let wheels = (0..4).filter(|&i| physics.riding.ground.parts[i].in_contact).count();
    let airborne = (200..300).contains(&state) && wheels == 0;
    let grinding = skater.player_input.physical.grinds.grinding_316 != 0;
    let balance = skater.player_state.state_flags.get(60 - 52).copied().unwrap_or(false);
    let intent = controls.intents.iter().find(|i| i.name == "Manual").map(|i| i.value);
    seen.held = if intent.is_some_and(|v| v != 0.0) { seen.held + dt } else { 0.0 };

    if let Some(p) = seen.pending.as_mut() {
        p.since += dt;
        p.intent_lost |= intent.is_none_or(|v| v == 0.0);
        if balance || p.since > WINDOW || airborne {
            let p = seen.pending.take().unwrap();
            let asked = p.intent.is_some_and(|v| v != 0.0);
            let reason = if balance {
                "granted"
            } else if !asked {
                "no_stick"
            } else if p.intent_lost {
                "stick_released"
            } else {
                "graph_declined"
            };
            let opt = |v: Option<f32>| v.map_or("none".to_string(), |v| format!("{v:.3}"));
            info!(
                "MANUAL_LANDING tick={} stick=[{:.2}, {:.2}] asked={} intent={:.2} held_before={:.2}s engage_time={} wheels_at_landing={} graph_state={} clip={} granted={} after={:.2}s result={reason} state={state}",
                p.tick, p.stick[0], p.stick[1], asked, p.intent.unwrap_or(0.0), p.held, opt(p.engage), p.wheels,
                p.graph_state, p.clip, balance, p.since,
            );
        }
    }
    if seen.was_air && !airborne && !grinding {
        let words = controls.controller.words();
        let motion = &skater.animation.motion.animation;
        let engage = motion
            .motion_intents
            .get("ManualEngageTime")
            .copied()
            .or_else(|| controls.named_intents.get("ManualEngageTime").copied());
        let current = skater.animation.motion_controller.frame.current;
        let name = current.and_then(|id| graphs.as_ref().and_then(|g| g.motion.binding.states.get(id as usize)).map(|s| s.name.clone()));
        seen.pending = Some(Pending {
            tick: physics.ticks,
            stick: [f32::from_bits(words[9]), f32::from_bits(words[10])],
            intent,
            engage,
            held: seen.held,
            wheels,
            since: 0.0,
            intent_lost: false,
            graph_state: format!("{}:{}", current.map_or("none".to_string(), |id| id.to_string()), name.as_deref().unwrap_or("?")),
            clip: motion.current_name.clone().unwrap_or_else(|| "none".into()),
        });
    }
    seen.was_air = airborne;
}
