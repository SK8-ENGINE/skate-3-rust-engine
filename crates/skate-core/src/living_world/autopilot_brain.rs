//! OURS, not retail: the player autopilot's layer over the NPC skater brain. It has no speed pull and no launches, so
//! it must bring its own speed to each recorded jump: docs/hails-additions/42-challenges.md#change-player-autopilot-autopilotrs-ours-not-retail-2026-11-08

use super::ai_record::{current_trajectory, EXT_HAS_TRAJECTORY};
use super::replay::{LineSource, ReplayJump, ReplayLine};

/// The autopilot's jump rules (data-driven; defaults are ours).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct JumpRules {
    /// How far ahead (recorded 60 Hz frames) a recorded jump raises the push target to its takeoff speed.
    pub look_ahead_frames: u32,
    /// Keep pushing toward the takeoff speed until the jump is this close (frames); retail stops 40 frames before.
    pub push_until_frames: u32,
    /// The recorded arc is used only at or above this share of its recorded flat takeoff speed, so it never adds speed.
    pub arc_speed_share: f32,
    /// This close to a jump (frames) and below `switch_share` of its takeoff speed, the autopilot leaves for another
    /// line instead of jumping slow...
    pub switch_frames: u32,
    pub switch_share: f32,
    /// ...but not closer than this (frames): at the takeoff itself it is too late to leave.
    pub switch_min_frames: u32,
    /// ...for a node 1 m or more ahead, within `switch_reach` (m, under the brain's 6 m lost radius; 12 m ahead lost it
    /// at once on the laptop), at most `switch_side` to the side, this close in height (m), heading its way, and whose
    /// recorded speed is at most `switch_speed_ratio` x its own (at least 3 m/s).
    pub switch_reach: f32,
    pub switch_side: f32,
    pub switch_height: f32,
    pub switch_speed_ratio: f32,
}

impl Default for JumpRules {
    fn default() -> Self {
        Self { look_ahead_frames: 240, push_until_frames: 12, arc_speed_share: 1.0, switch_frames: 45, switch_share: 0.85, switch_min_frames: 15, switch_reach: 5.0, switch_side: 4.0, switch_height: 2.5, switch_speed_ratio: 1.5 }
    }
}

/// The next recorded jump with an arc on the line: frames until it and its flat takeoff speed (m/s).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct JumpAhead {
    pub frames: u32,
    pub speed: f32,
}

/// Flat (x / z) speed of a recorded takeoff.
pub fn takeoff_speed(jump: &ReplayJump) -> f32 {
    jump.start_velocity[0].hypot(jump.start_velocity[2])
}

/// The jump at the cursor (0 frames) or the first later node with one, within `rules.look_ahead_frames`.
pub fn jump_ahead(line: &ReplayLine, node: u32, into: u32, rules: &JumpRules) -> Option<JumpAhead> {
    if let Some(jump) = current_trajectory(line, node, into) {
        return Some(JumpAhead { frames: 0, speed: takeoff_speed(jump) });
    }
    let mut ahead = -i64::from(into);
    for n in line.nodes.iter().skip(node as usize + 1) {
        ahead += i64::from(n.frames);
        if ahead >= i64::from(rules.look_ahead_frames) {
            return None;
        }
        if let Some(jump) = n.jump.and_then(|j| line.jumps.get(j as usize)).filter(|j| j.flags & EXT_HAS_TRAJECTORY != 0) {
            return Some(JumpAhead { frames: ahead.max(0) as u32, speed: takeoff_speed(jump) });
        }
    }
    None
}

/// The push target with a jump ahead: at least its takeoff speed.
pub fn push_target(target: f32, jump: Option<JumpAhead>) -> f32 {
    jump.map_or(target, |j| target.max(j.speed))
}

/// Whether the trick look-ahead may still cut the push: only once the jump is close or the speed is there.
pub fn trick_cuts_push(speed: f32, jump: Option<JumpAhead>, rules: &JumpRules) -> bool {
    jump.is_none_or(|j| speed >= j.speed || j.frames <= rules.push_until_frames)
}

/// Whether the recorded arc may launch the skater at this flat speed (never faster than the skater already is).
pub fn keeps_arc(flat_speed: f32, jump: &ReplayJump, rules: &JumpRules) -> bool {
    flat_speed >= takeoff_speed(jump) * rules.arc_speed_share
}

/// Too slow for the jump just ahead: time to leave the line.
pub fn should_switch(speed: f32, jump: Option<JumpAhead>, rules: &JumpRules) -> bool {
    jump.is_some_and(|j| (rules.switch_min_frames..=rules.switch_frames).contains(&j.frames) && speed < j.speed * rules.switch_share)
}

/// The closest node (side distance first) of another line that is ahead of the skater, heading its way (within 45
/// degrees), and whose own next jump (if any) this speed can make; `forward` is the flat unit direction of travel.
/// `only`: the challenge's own lines while one runs (a race never leaves its course).
pub fn switch_line(lines: &dyn LineSource, from: [u8; 16], position: [f32; 3], forward: [f32; 3], speed: f32, only: Option<&[[u8; 16]]>, rules: &JumpRules) -> Option<([u8; 16], u32)> {
    let mut best: Option<(f32, [u8; 16], u32)> = None;
    lines.for_each_line(&mut |line| {
        if line.id == from || only.is_some_and(|o| !o.contains(&line.id)) {
            return;
        }
        for (i, n) in line.nodes.iter().enumerate().take(line.nodes.len().saturating_sub(2)) {
            let d = [n.position[0] - position[0], n.position[1] - position[1], n.position[2] - position[2]];
            let ahead = d[0] * forward[0] + d[2] * forward[2];
            let side = (d[0] * forward[2] - d[2] * forward[0]).abs();
            let dist = d[0].hypot(d[2]);
            if ahead < 1.0 || dist > rules.switch_reach || side > rules.switch_side || d[1].abs() > rules.switch_height {
                continue;
            }
            let recorded = n.step[0].hypot(n.step[1]).hypot(n.step[2]) * super::ai_record::FRAMES_PER_SECOND;
            if recorded > speed.max(3.0) * rules.switch_speed_ratio {
                continue;
            }
            let run = n.step[0].hypot(n.step[2]);
            if run < 1e-4 || (n.step[0] * forward[0] + n.step[2] * forward[2]) / run < 0.7 {
                continue;
            }
            if jump_ahead(line, i as u32, 0, rules).is_some_and(|j| speed < j.speed * rules.switch_share) {
                continue;
            }
            if best.is_none_or(|b| dist < b.0) {
                best = Some((dist, line.id, i as u32));
            }
        }
    });
    best.map(|(_, line, node)| (line, node))
}

/// The autopilot's brake (ours: retail brakes only past ~3.3x the line's speed and its NPCs slow down through the speed
/// pull, which the autopilot has not): the slowest recorded speed within `look_ahead_frames` of the line, so it slows
/// before a slow section instead of arriving too fast; it brakes above `factor`'s inverse x that speed.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BrakeRules {
    pub look_ahead_frames: u32,
    /// Retail's brake factor for the autopilot (`BrakeSettings::factor`; retail 0.3): 0.8 brakes above 1.25x.
    pub factor: f32,
}

impl Default for BrakeRules {
    fn default() -> Self {
        Self { look_ahead_frames: 60, factor: 0.8 }
    }
}

/// The slowest recorded speed (m/s) from the cursor through `rules.look_ahead_frames` of the line.
pub fn approach_speed(line: &ReplayLine, node: u32, into: u32, rules: &BrakeRules) -> Option<f32> {
    let speed = |n: &super::replay::ReplayNode| n.step[0].hypot(n.step[1]).hypot(n.step[2]) * super::ai_record::FRAMES_PER_SECOND;
    let mut slowest = line.nodes.get(node as usize).map(speed)?;
    let mut ahead = -i64::from(into);
    for n in line.nodes.iter().skip(node as usize + 1) {
        ahead += i64::from(n.frames);
        if ahead > i64::from(rules.look_ahead_frames) {
            break;
        }
        slowest = slowest.min(speed(n));
    }
    Some(slowest)
}

/// The autopilot gets up from a bail like a player hitting reset (ours): back on its line this long after the bail,
/// not after the NPC skaters' 5 s lie (they keep theirs; user, 2026-11-09).
pub const BAIL_RESET_SECONDS: f32 = 2.0;

/// Pumping down a slope (ours: retail AI never pumps; the player's auto pump gesture, `AutoPumpAngle` 0 and
/// `AutoPumpMag` held, so the stock crouch node crouches and pumps as it does for a player).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PumpRules {
    /// The line's slope (rise over run) at or below which it pumps.
    pub slope: f32,
    /// At least this fast (m/s).
    pub min_speed: f32,
    /// Ticks the stick is held forward per pump, then released (a new pump needs the magnitude to rise again).
    pub hold_ticks: u32,
    pub release_ticks: u32,
}

impl Default for PumpRules {
    fn default() -> Self {
        Self { slope: -0.04, min_speed: 1.5, hold_ticks: 40, release_ticks: 10 }
    }
}

/// This tick's `AutoPumpMag` (with `AutoPumpAngle` 0), or `None` when it does not pump; `cycle` counts the ticks of
/// the current pumping stretch (reset when it stops).
pub fn pump_magnitude(cycle: &mut u32, slope: f32, speed: f32, pushing: bool, rules: &PumpRules) -> Option<f32> {
    if slope > rules.slope || speed < rules.min_speed || pushing {
        *cycle = 0;
        return None;
    }
    let t = *cycle % (rules.hold_ticks + rules.release_ticks).max(1);
    *cycle += 1;
    Some(if t < rules.hold_ticks { 1.0 } else { 0.0 })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::living_world::replay::ReplayNode;

    fn line(jump_at: usize) -> ReplayLine {
        let nodes = (0..10)
            .map(|i| ReplayNode { position: [0.0; 3], step: [0.0; 3], board: [128; 4], skater: [128; 4], frames: if i == 0 { 0 } else { 30 }, event: 0, flags: 0, jump: (i == jump_at).then_some(0), width: [0; 2] })
            .collect();
        let jump = ReplayJump { start_position: [0.0; 3], start_velocity: [6.0, 3.0, 8.0], offset: [0.0; 3], trick: 0, spins: 0, flags: EXT_HAS_TRAJECTORY };
        ReplayLine { id: [0; 16], flags: 0, skill: -1, nodes, jumps: vec![jump], groups: Vec::new() }
    }

    #[test]
    fn a_jump_ahead_raises_the_push_target_to_its_takeoff_speed() {
        let rules = JumpRules::default();
        let ahead = jump_ahead(&line(5), 1, 10, &rules).unwrap();
        assert_eq!(ahead, JumpAhead { frames: 110, speed: 10.0 });
        assert_eq!(push_target(7.0, Some(ahead)), 10.0);
        assert_eq!(push_target(12.0, Some(ahead)), 12.0);
        assert!(jump_ahead(&line(9), 0, 0, &rules).is_none(), "240 frames is the look-ahead");
    }

    #[test]
    fn the_trick_cut_waits_until_the_jump_is_close_or_the_speed_is_there() {
        let rules = JumpRules::default();
        let far = Some(JumpAhead { frames: 30, speed: 10.0 });
        assert!(!trick_cuts_push(8.0, far, &rules));
        assert!(trick_cuts_push(10.5, far, &rules));
        assert!(trick_cuts_push(8.0, Some(JumpAhead { frames: 12, speed: 10.0 }), &rules));
        assert!(trick_cuts_push(8.0, None, &rules));
    }

    fn straight(id: u8, x: f32, jump_at: Option<usize>, takeoff: f32) -> ReplayLine {
        let mut l = line(jump_at.unwrap_or(99));
        l.id = [id; 16];
        l.jumps[0].start_velocity = [0.0, 3.0, takeoff];
        for (i, n) in l.nodes.iter_mut().enumerate() {
            n.position = [x, 0.0, i as f32 * 4.0];
            n.step = [0.0, 0.0, 4.0 / 30.0];
        }
        l
    }

    #[test]
    fn a_skater_too_slow_for_the_jump_ahead_switches_to_a_line_it_can_ride() {
        let rules = JumpRules::default();
        assert!(should_switch(7.0, Some(JumpAhead { frames: 40, speed: 10.0 }), &rules));
        assert!(!should_switch(7.0, Some(JumpAhead { frames: 60, speed: 10.0 }), &rules), "still time to push");
        assert!(!should_switch(9.0, Some(JumpAhead { frames: 40, speed: 10.0 }), &rules));
        assert!(!should_switch(7.0, Some(JumpAhead { frames: 10, speed: 10.0 }), &rules), "too late to leave");
        let mut lines = std::collections::BTreeMap::new();
        for l in [straight(1, 0.0, Some(4), 10.0), straight(2, 2.0, Some(5), 12.0), straight(3, -4.0, None, 0.0), straight(4, 1.0, None, 0.0)] {
            lines.insert(l.id, l);
        }
        lines.get_mut(&[4; 16]).unwrap().nodes.iter_mut().for_each(|n| n.step = [0.0, 0.0, -4.0 / 30.0]);
        // Line 2 is nearer but jumps at 12 m/s; line 4 runs the other way; line 3 has no jump.
        assert_eq!(switch_line(&lines, [1; 16], [0.0, 0.0, 6.0], [0.0, 0.0, 1.0], 7.0, None, &rules), Some(([3; 16], 2)));
        assert_eq!(switch_line(&lines, [1; 16], [0.0, 0.0, 6.0], [0.0, 0.0, 1.0], 11.0, None, &rules), Some(([2; 16], 2)));
        // In a race only the race's own lines: here line 2 alone, too fast at 7 m/s, so none.
        assert_eq!(switch_line(&lines, [1; 16], [0.0, 0.0, 6.0], [0.0, 0.0, 1.0], 7.0, Some(&[[1; 16], [2; 16]]), &rules), None);
    }

    #[test]
    fn it_pumps_down_slopes_in_hold_release_cycles_and_not_while_pushing() {
        let rules = PumpRules::default();
        let mut cycle = 0;
        assert_eq!(pump_magnitude(&mut cycle, 0.0, 5.0, false, &rules), None, "flat");
        let seq: Vec<f32> = (0..51).filter_map(|_| pump_magnitude(&mut cycle, -0.2, 5.0, false, &rules)).collect();
        assert_eq!((seq[0], seq[39], seq[40], seq[49], seq[50]), (1.0, 1.0, 0.0, 0.0, 1.0));
        assert_eq!(pump_magnitude(&mut cycle, -0.2, 5.0, true, &rules), None, "pushing");
        assert_eq!(cycle, 0);
        assert_eq!(pump_magnitude(&mut cycle, -0.2, 1.0, false, &rules), None, "too slow");
    }

    #[test]
    fn the_approach_speed_is_the_slowest_recorded_speed_just_ahead() {
        let mut l = straight(1, 0.0, None, 0.0);
        l.nodes[3].step = [0.0, 0.0, 0.05];
        let rules = BrakeRules::default();
        assert!((approach_speed(&l, 1, 0, &rules).unwrap() - 3.0).abs() < 1e-4, "node 3 is 60 frames ahead");
        assert!((approach_speed(&l, 4, 0, &rules).unwrap() - 8.0).abs() < 1e-4);
    }

    #[test]
    fn the_recorded_arc_never_launches_a_slower_skater() {
        let rules = JumpRules::default();
        let line = line(5);
        assert!(!keeps_arc(9.9, &line.jumps[0], &rules));
        assert!(keeps_arc(10.0, &line.jumps[0], &rules));
    }
}
