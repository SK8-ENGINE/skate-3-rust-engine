//! The NPC skater controller's sub-modes for the obstacle avoider's modes 6 and 7 (retail TU3, evidence only;
//! re-implemented; `.local/research/npc/b66-npc-modes-6-7.md`, main checked `sub_8246F818` cases 3 / 5):
//! - mode 6 (a low prop within 1 s): `sub_8246D560` sets controller sub-mode 4 (`ctrl+568`) when below 4; while 4
//!   the recorded node action is dropped, the speed shape is skipped and no grab / trick is posted;
//! - mode 7 (blocked by a prop within 1.5 s, cap under 0.1): `sub_8246F938` (not airborne) sets sub-mode 3 and a
//!   one-shot reposition request (`ctrl+941`) on the first tick; a second tick still in mode 7 sets sub-mode 5,
//!   remembers the prop's path node (`ctrl+756`) and posts `WipeOutRequest` 1.0 once (key 0x830BECC4); later ticks
//!   post nothing;
//! - `sub_8246FE38` at the end of every controller tick: a pending request (offline only) re-places the skater on
//!   its line, at most once per 60 clock units: in sub-mode 3 at cursor + 5 (or the prop's node when further in
//!   mode 7), else at the cursor; then + 2; a remembered prop node + 2 within 1..19 nodes ahead wins; clamped to the
//!   line. A successful move resets the controller (sub-mode 0). The request is cleared either way.
//!
//! Multiplayer: [`ControllerState`] is plain per-NPC data; the host runs it.

/// Retail numbers; every field is data a mod can override.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ControllerSettings {
    /// Nodes ahead of the cursor for the sub-mode 3 reposition (5).
    pub reposition_ahead: u32,
    /// Nodes past the chosen node (2).
    pub reposition_past: u32,
    /// A remembered prop node wins when it lies 1..this nodes past the choice (20, exclusive).
    pub remembered_window: u32,
    /// Clock units between two repositions (60; the clock unit is not decoded, b66).
    pub reposition_interval: u64,
    /// The `WipeOutRequest` value (1.0, `0x8231A844`).
    pub wipe_out: f32,
    /// The safe node picker (`sub_82455728`): a node is good when more than `safe_jump_frames` (45) recorded 60 Hz
    /// frames have passed since a jump marker and more than `safe_flag_frames` (15) since a flag-0x4 node, its step
    /// slope y^2 / (x^2 + z^2) is under `safe_slope` (1.0, `0x8231A844`; 0 when the horizontal length^2 is at most
    /// `0x82063A48` 0.001) and its trick class is 0, 4 or 8; the first of `safe_run` (3) good nodes in a row wins.
    pub safe_jump_frames: u32,
    pub safe_flag_frames: u32,
    pub safe_slope: f32,
    pub safe_run: u32,
}

impl Default for ControllerSettings {
    fn default() -> Self {
        Self { reposition_ahead: 5, reposition_past: 2, remembered_window: 20, reposition_interval: 60, wipe_out: 1.0, safe_jump_frames: 45, safe_flag_frames: 15, safe_slope: 1.0, safe_run: 3 }
    }
}

/// `ctrl+568` values used here.
pub mod sub_mode {
    pub const NORMAL: u8 = 0;
    pub const STEP_OFF: u8 = 3;
    pub const LOW_PROP: u8 = 4;
    pub const STEP_OFF_BAIL: u8 = 5;
}

/// The controller fields this logic owns (`+568`, `+941`, `+943` / `+756`, `+912`).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct ControllerState {
    pub sub_mode: u8,
    pub reposition_request: bool,
    pub remembered_node: Option<u32>,
    pub last_reposition: Option<u64>,
}

/// `sub_8246D560` for mode 6: enter sub-mode 4 (its exit is not decoded yet, b66).
pub fn enter_low_prop(state: &mut ControllerState) {
    if state.sub_mode < sub_mode::LOW_PROP {
        state.sub_mode = sub_mode::LOW_PROP;
    }
}

/// `sub_8246F938` for one tick; returns the `WipeOutRequest` value to post, if any. `online` is the flag at
/// `0x830CFE5C`+608 (meaning open; offline 0 runs the reposition step first).
pub fn step_off(state: &mut ControllerState, s: &ControllerSettings, mode_7: bool, airborne: bool, online: bool, prop_node: u32) -> Option<f32> {
    if !mode_7 || airborne {
        return None;
    }
    if !online && state.sub_mode < sub_mode::STEP_OFF {
        state.sub_mode = sub_mode::STEP_OFF;
        state.reposition_request = true;
        return None;
    }
    if state.sub_mode < sub_mode::STEP_OFF_BAIL {
        state.sub_mode = sub_mode::STEP_OFF_BAIL;
        state.remembered_node = Some(prop_node);
        return Some(s.wipe_out);
    }
    None
}

/// `sub_8246FE38`'s node choice.
pub fn reposition_node(state: &ControllerState, s: &ControllerSettings, mode_7: bool, cursor: u32, prop_node: u32, node_count: u32) -> u32 {
    let mut node = cursor;
    if state.sub_mode == sub_mode::STEP_OFF {
        let mut d = s.reposition_ahead;
        if mode_7 && prop_node > cursor && prop_node - cursor > d {
            d = prop_node - cursor;
        }
        node = cursor + d;
    }
    node += s.reposition_past;
    if let Some(r) = state.remembered_node {
        let r = r + s.reposition_past;
        if r > node && r - node < s.remembered_window {
            node = r;
        }
    }
    node.min(node_count.saturating_sub(1))
}

/// `sub_82455348`: the node's jump is a ground trick (category 5 grind / slide, 8 manual, 9 powerslide, or 4 grab
/// for the ground grabs coffin 60 and gnd_bsgrab / gnd_dblgrab / gnd_fsgrab 67 to 69; `sub_824545F0` reads the
/// category, table `0x820862A8` +16).
fn ground_trick(line: &super::replay::ReplayLine, node: &super::replay::ReplayNode) -> bool {
    let Some(trick) = node.jump.and_then(|j| line.jumps.get(j as usize)).map(|j| j.trick) else { return false };
    match crate::scoring::catalog::category(trick) {
        Some(5 | 8 | 9) => true,
        Some(4) => matches!(trick, 60 | 67 | 68 | 69),
        _ => false,
    }
}

/// `sub_82455728(line, start)`: the safe node to re-attach at, from `start` on (the counters are primed over the
/// nodes before it); the last node when none qualifies.
pub fn safe_node(line: &super::replay::ReplayLine, start: u32, s: &ControllerSettings) -> u32 {
    use super::replay::node_events;
    const RESET: u32 = 7200;
    let (mut since_jump, mut since_flag, mut class) = (RESET, RESET, 0usize);
    let mut count = |n: &super::replay::ReplayNode| {
        let jump = n.jump.and_then(|j| line.jumps.get(j as usize));
        // Jump marker (ext +39 bit 0), a ground trick's start, else count the frames.
        since_jump = if jump.is_some_and(|j| j.flags & 1 != 0) {
            0
        } else if n.event == node_events::START_TRICK && ground_trick(line, n) {
            RESET
        } else {
            since_jump + u32::from(n.frames)
        };
        since_flag = if n.flags & 0x4 != 0 { 0 } else { since_flag + u32::from(n.frames) };
        if n.event == node_events::START_TRICK {
            class = jump.and_then(|j| crate::scoring::catalog::category(j.trick)).unwrap_or(0);
        } else if n.event == node_events::END_TRICK {
            class = 0;
        }
        (since_jump, since_flag, class)
    };
    let nodes = &line.nodes;
    for n in nodes.iter().take(start as usize) {
        count(n);
    }
    let mut run = 0;
    let mut first = 0u32;
    for i in start as usize..nodes.len() {
        let n = &nodes[i];
        let (a, b, c) = count(n);
        let h2 = n.step[0] * n.step[0] + n.step[2] * n.step[2];
        let slope = if h2 > 0.001 { n.step[1] * n.step[1] / h2 } else { 0.0 };
        let good = a > s.safe_jump_frames && b > s.safe_flag_frames && slope < s.safe_slope && matches!(c, 0 | 4 | 8);
        if !good {
            run = 0;
            continue;
        }
        if run == 0 {
            first = i as u32;
        }
        run += 1;
        if run >= s.safe_run {
            return first;
        }
    }
    (nodes.len() as u32).saturating_sub(1)
}

/// `sub_8246FE38`: the node to move to when the one-shot request may run now (offline, throttled); the request is
/// cleared either way. The caller reports success with [`reposition_done`]. The chosen node then goes through
/// [`safe_node`] before the swap to controller B.
pub fn take_reposition(state: &mut ControllerState, s: &ControllerSettings, online: bool, now: u64, mode_7: bool, cursor: u32, prop_node: u32, node_count: u32) -> Option<u32> {
    let pending = std::mem::take(&mut state.reposition_request);
    if !pending || online || state.last_reposition.is_some_and(|last| now < last + s.reposition_interval) {
        return None;
    }
    let node = reposition_node(state, s, mode_7, cursor, prop_node, node_count);
    state.last_reposition = Some(now);
    Some(node)
}

/// A successful move resets the controller (sub-mode 0; the throttle time is kept).
pub fn reposition_done(state: &mut ControllerState) {
    *state = ControllerState { last_reposition: state.last_reposition, ..Default::default() };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mode_7_requests_a_reposition_then_wipes_out_once() {
        let s = ControllerSettings::default();
        let mut c = ControllerState::default();
        // Airborne: nothing.
        assert_eq!(step_off(&mut c, &s, true, true, false, 40), None);
        assert_eq!(c, ControllerState::default());
        assert_eq!(step_off(&mut c, &s, true, false, false, 40), None);
        assert!(c.reposition_request && c.sub_mode == sub_mode::STEP_OFF);
        assert_eq!(step_off(&mut c, &s, true, false, false, 40), Some(1.0));
        assert_eq!((c.sub_mode, c.remembered_node), (sub_mode::STEP_OFF_BAIL, Some(40)));
        assert_eq!(step_off(&mut c, &s, true, false, false, 40), None);
        // Online: straight to the wipe out.
        let mut c = ControllerState::default();
        assert_eq!(step_off(&mut c, &s, true, false, true, 40), Some(1.0));
    }

    #[test]
    fn the_reposition_node_follows_the_retail_choice() {
        let s = ControllerSettings::default();
        let step3 = ControllerState { sub_mode: sub_mode::STEP_OFF, ..Default::default() };
        assert_eq!(reposition_node(&step3, &s, true, 10, 12, 100), 17);
        assert_eq!(reposition_node(&step3, &s, true, 10, 30, 100), 32);
        assert_eq!(reposition_node(&step3, &s, true, 10, 30, 20), 19);
        let other = ControllerState { sub_mode: sub_mode::NORMAL, remembered_node: Some(20), ..Default::default() };
        assert_eq!(reposition_node(&other, &s, false, 10, 0, 100), 22);
        let far = ControllerState { remembered_node: Some(40), ..other };
        assert_eq!(reposition_node(&far, &s, false, 10, 0, 100), 12);
    }

    #[test]
    fn the_reposition_is_one_shot_and_throttled() {
        let s = ControllerSettings::default();
        let mut c = ControllerState { sub_mode: sub_mode::STEP_OFF, reposition_request: true, ..Default::default() };
        assert_eq!(take_reposition(&mut c, &s, false, 100, true, 10, 12, 100), Some(17));
        reposition_done(&mut c);
        assert_eq!((c.sub_mode, c.last_reposition), (sub_mode::NORMAL, Some(100)));
        c.reposition_request = true;
        assert_eq!(take_reposition(&mut c, &s, false, 130, false, 10, 12, 100), None);
        assert!(!c.reposition_request);
        c.reposition_request = true;
        assert_eq!(take_reposition(&mut c, &s, false, 160, false, 10, 12, 100), Some(12));
        // Online: never.
        c.reposition_request = true;
        assert_eq!(take_reposition(&mut c, &s, true, 1000, false, 10, 12, 100), None);
    }

    #[test]
    fn mode_6_enters_low_prop_without_lowering_a_higher_sub_mode() {
        let mut c = ControllerState::default();
        enter_low_prop(&mut c);
        assert_eq!(c.sub_mode, sub_mode::LOW_PROP);
        let mut c = ControllerState { sub_mode: sub_mode::STEP_OFF_BAIL, ..Default::default() };
        enter_low_prop(&mut c);
        assert_eq!(c.sub_mode, sub_mode::STEP_OFF_BAIL);
    }

    fn node(frames: u8, event: u8, flags: u8, jump: Option<u32>, step: [f32; 3]) -> crate::living_world::replay::ReplayNode {
        crate::living_world::replay::ReplayNode { position: [0.0; 3], step, board: [128; 4], skater: [128; 4], frames, event, flags, jump, width: [50, 50] }
    }

    fn jump(trick: i16, flags: u8) -> crate::living_world::replay::ReplayJump {
        crate::living_world::replay::ReplayJump { start_position: [0.0; 3], start_velocity: [0.0; 3], offset: [0.0; 3], trick, spins: 0, flags }
    }

    #[test]
    fn the_safe_node_is_the_first_of_three_calm_nodes_past_tricks_flags_and_slopes() {
        use crate::living_world::replay::{node_events, ReplayLine};
        let s = ControllerSettings::default();
        let flat = [0.0, 0.0, 0.1];
        // 0: a jump marker (frames reset); 1..4: 10 frames each (40, not past 45); 5: a flag-0x4 node; 6..: calm.
        let mut nodes = vec![node(10, 0, 0, Some(0), flat)];
        nodes.extend((0..4).map(|_| node(10, 0, 0, None, flat)));
        nodes.push(node(10, 0, 0x4, None, flat));
        nodes.extend((0..6).map(|_| node(10, 0, 0, None, flat)));
        let line = ReplayLine { id: [0; 16], flags: 0, skill: -1, nodes, jumps: vec![jump(128, 1)], groups: Vec::new() };
        // Node 5 has jump frames 50 but flag frames 0; node 6: 60 / 10; node 7: 70 / 20 is the first good one.
        assert_eq!(safe_node(&line, 0, &s), 7);
        assert_eq!(safe_node(&line, 8, &s), 8);
        // A steep step (slope >= 1) breaks the run.
        let mut steep = line.clone();
        steep.nodes[8].step = [0.0, 0.2, 0.1];
        assert_eq!(safe_node(&steep, 0, &s), 9);
        // A trick started at node 7: an ollie (class 1) keeps every later node unsafe until its end event (none
        // here: the last node); a manual (class 8, nosemanual 2) is calm.
        let mut ollie = line.clone();
        ollie.jumps.push(jump(128, 0));
        ollie.nodes[7].event = node_events::START_TRICK;
        ollie.nodes[7].jump = Some(1);
        assert_eq!(safe_node(&ollie, 0, &s), 11);
        let mut manual = ollie.clone();
        manual.jumps[1].trick = 2;
        assert_eq!(safe_node(&manual, 0, &s), 7);
        let none = ReplayLine { nodes: vec![node(1, 0, 0x4, None, flat); 4], ..line.clone() };
        assert_eq!(safe_node(&none, 0, &s), 3);
    }
}
