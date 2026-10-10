//! The ActionGraph signals an AI skater's PathController posts so a simulated NPC skater rides
//! into its recorded tricks like a player would (doc 26 "Simulated NPC skaters do their tricks").
//! Retail writes named signals into the same intent map the player's pad listener fills; no pad
//! bits or stick gestures (`.local/research/npc/m7-ai-trick-signals.md`, main checked
//! `sub_824691A0` and the signal addresses).
//!
//! Retail (TU3, evidence only; re-implemented):
//! - anticipation `sub_824691A0` (each tick from the node advance `sub_8246D560`): within 3.0 m of
//!   the line (`pc+828`, `0x82063B08`) it looks up to 60 recorded frames past the cursor for a
//!   start-trick node (`sub_82456550`); an ollie or flip there (category 1 / 2) sets `AnticMag`
//!   1.0 and `AnticAngle` 0, or pi for a nollie (the recorded trick, `sub_82469048`);
//!   `sub_824696B8` posts them (and `Crouch` on a crouched node outside a trick, `Manual` before a
//!   manual) when the skater faces along the line (under pi/4, `0x821A9C28`) [code, the posting
//!   gates partly inferred];
//! - dispatch `sub_8246A2E0`: on the tick the committed cursor reaches the start-trick node it
//!   posts `Trick`, the trick's scorable name, `GestureSpeed`, `TrickHeight` and
//!   `DontMirrorTrick`, all 1.0 (the graph enters the trick on `HasAGIntent <name>`, names
//!   compare case-insensitively); only the newest event node of the tick, none when the cursor
//!   crossed more than 3 nodes (1 right after spawn).

use super::replay::{node_events, node_flags, ReplayLine};
use crate::scoring::catalog;

/// Retail values (data-driven, mod-tunable through the game's settings).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SignalSettings {
    /// Anticipation only this close to the line, metres (`0x82063B08`, 3.0).
    pub anticipation_distance: f32,
    /// Look-ahead in recorded 60 Hz frames (`sub_824691A0`, 60).
    pub anticipation_frames: u32,
    /// Posted only while the skater faces the line within this angle (`0x821A9C28`, pi/4).
    pub anticipation_heading: f32,
    /// Event nodes are skipped when the cursor crossed more nodes than this in one tick (3).
    pub max_crossed_nodes: u32,
    /// Mode 4 (skitch): the GrabWorld value posted every tick (`sub_8246FA30`, `0x8231A844`, 1.0; b65).
    pub skitch_grab: f32,
}

impl Default for SignalSettings {
    fn default() -> Self {
        Self { anticipation_distance: 3.0, anticipation_frames: 60, anticipation_heading: core::f32::consts::FRAC_PI_4, max_crossed_nodes: 3, skitch_grab: 1.0 }
    }
}

/// One named signal for the ActionGraph intent map.
pub type Signal = (String, f32);

/// What the skater looks like to the signal writer this tick.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SignalInput {
    /// The cursor's node and frames into its segment.
    pub node: u32,
    pub frame_in_segment: u32,
    /// The node the dispatcher handled last (`pc+820`); `None` right after spawn.
    pub last_node: Option<u32>,
    /// Distance of the skater from the line, metres (`pc+828`).
    pub off_line: f32,
    /// Angle between the skater's facing and the line, radians.
    pub heading_error: f32,
    /// A trick span is open (no new anticipation, no crouch).
    pub in_trick: bool,
}

/// The first start-trick node within `frames` recorded frames after (`node`, `into`)
/// (`sub_82456550`).
pub fn upcoming_start(line: &ReplayLine, node: u32, into: u32, frames: u32) -> Option<u32> {
    let mut budget = i64::from(frames) + i64::from(into);
    for i in (node as usize + 1)..line.nodes.len() {
        budget -= i64::from(line.nodes[i].frames);
        if budget < 0 {
            return None;
        }
        if line.nodes[i].event == node_events::START_TRICK {
            return Some(i as u32);
        }
    }
    None
}

/// The signals of this tick. `chosen` gives the trick started at a start-trick node (the npc_tricks
/// choice the replay cursor made; the recorded trick when it has none).
pub fn signals(line: &ReplayLine, s: &SignalSettings, input: &SignalInput, chosen: &dyn Fn(u32) -> i16) -> Vec<Signal> {
    let mut out = Vec::new();
    // Anticipation.
    if input.off_line <= s.anticipation_distance && !input.in_trick && input.heading_error.abs() < s.anticipation_heading {
        if let Some(n) = upcoming_start(line, input.node, input.frame_in_segment, s.anticipation_frames) {
            let recorded = line.node_trick(&line.nodes[n as usize]);
            match catalog::category(recorded) {
                Some(1 | 2) => {
                    out.push(("AnticMag".into(), 1.0));
                    let angle = if super::npc_tricks::is_nollie(recorded) { core::f32::consts::PI } else { 0.0 };
                    out.push(("AnticAngle".into(), angle));
                }
                Some(8) => out.push(("Manual".into(), 1.0)),
                _ => {}
            }
        }
    }
    if !input.in_trick && line.nodes.get(input.node as usize).is_some_and(|n| n.flags & node_flags::CROUCHED != 0) {
        out.push(("Crouch".into(), 1.0));
    }
    // Dispatch: the newest start-trick node crossed this tick.
    let limit = if input.last_node.is_none() { 1 } else { s.max_crossed_nodes };
    let from = input.last_node.map_or(input.node, |l| l + 1);
    if input.node >= from && input.node + 1 - from <= limit {
        let newest = (from..=input.node).rev().find(|&n| line.nodes.get(n as usize).is_some_and(|x| x.event == node_events::START_TRICK));
        if let Some(n) = newest {
            let trick = chosen(n);
            if let Some((name, ..)) = usize::try_from(trick).ok().and_then(|i| catalog::IDENTIFIERS.get(i)) {
                out.push(("Trick".into(), 1.0));
                out.push(((*name).to_string(), 1.0));
                out.push(("GestureSpeed".into(), 1.0));
                out.push(("TrickHeight".into(), 1.0));
                out.push(("DontMirrorTrick".into(), 1.0));
            }
        }
    }
    out
}

/// Drop the trick dispatch's signals (the tail from `Trick`).
pub fn drop_trick_dispatch(out: &mut Vec<Signal>) {
    if let Some(i) = out.iter().position(|x| x.0 == "Trick") {
        out.truncate(i);
    }
}

/// Mode 4 of the obstacle avoider (`sub_8246FA30` at 0x8246FC78; b65): while the skater is not airborne the
/// controller posts GrabWorld every tick (the player's grab intent, so the riding skitch query and state 104 follow);
/// while it posts it or the skater is skitching (state 104) the trick dispatch is skipped (its signals dropped).
pub fn apply_skitch_mode(out: &mut Vec<Signal>, s: &SignalSettings, skitch_mode: bool, airborne: bool, skitching: bool) {
    let grab = skitch_mode && !airborne;
    if grab || skitching {
        drop_trick_dispatch(out);
    }
    if grab {
        out.push(("GrabWorld".into(), s.skitch_grab));
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn mode_4_grabs_and_skips_the_trick_dispatch() {
        let s = super::SignalSettings::default();
        let base: Vec<super::Signal> = vec![("Crouch".into(), 1.0), ("Trick".into(), 1.0), ("360flip".into(), 1.0), ("GestureSpeed".into(), 1.0)];
        let mut v = base.clone();
        super::apply_skitch_mode(&mut v, &s, true, false, false);
        assert_eq!(v, vec![("Crouch".into(), 1.0), ("GrabWorld".into(), 1.0)]);
        // Airborne: no grab, tricks kept.
        let mut v = base.clone();
        super::apply_skitch_mode(&mut v, &s, true, true, false);
        assert_eq!(v, base);
        // Skitching without mode 4: tricks dropped, no grab.
        let mut v = base.clone();
        super::apply_skitch_mode(&mut v, &s, false, false, true);
        assert_eq!(v, vec![("Crouch".into(), 1.0)]);
    }

    use super::*;
    use crate::living_world::replay::{ReplayJump, ReplayNode};

    /// Nodes 4 frames apart; a start-trick node at `at` carrying `trick`.
    fn line(at: usize, trick: i16, flags_at: &[(usize, u8)]) -> ReplayLine {
        ReplayLine {
            id: [0; 16],
            flags: 4,
            skill: 0,
            nodes: (0..40)
                .map(|i| ReplayNode {
                    position: [0.0, 0.0, i as f32],
                    step: [0.0; 3],
                    board: [128, 128, 128, 255],
                    skater: [128, 128, 128, 255],
                    frames: 4,
                    event: if i == at { node_events::START_TRICK } else { 0 },
                    flags: flags_at.iter().find(|f| f.0 == i).map_or(0, |f| f.1),
                    jump: (i == at).then_some(0),
                    width: [50, 50],
                })
                .collect(),
            jumps: vec![ReplayJump { start_position: [0.0; 3], start_velocity: [0.0; 3], offset: [0.0; 3], trick, spins: 0, flags: 0 }],
            groups: vec![],
        }
    }

    fn at(node: u32, last: Option<u32>) -> SignalInput {
        SignalInput { node, frame_in_segment: 0, last_node: last, off_line: 0.2, heading_error: 0.1, in_trick: false }
    }

    fn names(v: &[Signal]) -> Vec<&str> {
        v.iter().map(|s| s.0.as_str()).collect()
    }

    #[test]
    fn an_ollie_within_a_second_is_anticipated_then_started() {
        // 360flip (id 85, category 2) at node 20: 80 frames from node 0, 60 from node 5.
        let l = line(20, 85, &[]);
        let s = SignalSettings::default();
        assert!(signals(&l, &s, &at(0, Some(0)), &|_| 85).is_empty());
        let near = signals(&l, &s, &at(5, Some(4)), &|_| 85);
        assert_eq!(names(&near), ["AnticMag", "AnticAngle"]);
        assert_eq!(near[1].1, 0.0);
        // Too far off the line or facing away: nothing.
        assert!(signals(&l, &s, &SignalInput { off_line: 3.5, ..at(5, Some(4)) }, &|_| 85).is_empty());
        assert!(signals(&l, &s, &SignalInput { heading_error: 1.0, ..at(5, Some(4)) }, &|_| 85).is_empty());
        // Reaching the node: the trick, by its scorable name.
        let go = signals(&l, &s, &at(20, Some(19)), &|_| 85);
        assert_eq!(names(&go), ["Trick", "360flip", "GestureSpeed", "TrickHeight", "DontMirrorTrick"]);
    }

    #[test]
    fn the_chosen_trick_is_started_and_a_nollie_is_anticipated_backwards() {
        let nollie = catalog::IDENTIFIERS.iter().position(|t| t.0.starts_with('n') && t.2 == 1).unwrap() as i16;
        let l = line(10, nollie, &[]);
        let s = SignalSettings::default();
        let v = signals(&l, &s, &at(5, Some(4)), &|_| nollie);
        assert_eq!(v[1], ("AnticAngle".to_string(), core::f32::consts::PI));
        let go = signals(&l, &s, &at(10, Some(9)), &|_| 85);
        assert_eq!(go[1].0, "360flip");
    }

    #[test]
    fn crossing_too_many_nodes_skips_the_event_and_crouch_follows_the_node() {
        let l = line(10, 85, &[(3, node_flags::CROUCHED)]);
        let s = SignalSettings::default();
        assert!(names(&signals(&l, &s, &at(10, Some(5)), &|_| 85)).iter().all(|n| *n != "Trick"));
        assert!(names(&signals(&l, &s, &at(10, Some(7)), &|_| 85)).contains(&"Trick"));
        assert!(names(&signals(&l, &s, &at(3, Some(2)), &|_| 85)).contains(&"Crouch"));
        assert!(!names(&signals(&l, &s, &SignalInput { in_trick: true, ..at(3, Some(2)) }, &|_| 85)).contains(&"Crouch"));
    }
}
