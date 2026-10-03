//! Replaying a recorded audio state log (`state_log.rs`, `SKATE_AUDIO_STATE_LOG`) or an e2e
//! scenario (`tools/audio-e2e/scenarios.py`) as the per-step audio state the game publishes: the
//! headless e2e render (`e2e.rs`) and the ghost NPC skater (`ghost.rs`) both use it. Moved
//! unchanged out of `e2e.rs` (2026-10-03, world audio hook-in; the e2e renders are byte-identical).
use std::collections::HashMap;

use skate_audio::player::AudioState;
use skate_audio::player::state::material_of_tag;

use super::skate_events::Riding;

pub(crate) struct Row(pub(crate) HashMap<String, f64>);
impl Row {
    pub(crate) fn f(&self, k: &str) -> f32 {
        self.0.get(k).copied().unwrap_or(0.0) as f32
    }
    pub(crate) fn i(&self, k: &str) -> i64 {
        self.0.get(k).copied().unwrap_or(0.0) as i64
    }
}

/// Read a log without panicking (the ghost skater's input): `Err` names the first bad line. The
/// rows must all parse (memory: data with malformed lines is unusable, never parsed around).
pub(crate) fn try_read(text: &str) -> Result<Vec<Row>, String> {
    let mut lines = text.lines();
    let cols: Vec<String> = lines.next().ok_or("empty log")?.split('\t').map(str::to_owned).collect();
    if !cols.iter().any(|c| c == "speed") || !cols.iter().any(|c| c == "board_x") {
        return Err("not an audio state log (no speed / board_x columns)".into());
    }
    let mut rows = Vec::new();
    for (n, l) in lines.enumerate() {
        let values: Result<Vec<f64>, _> = l.split('\t').map(str::parse::<f64>).collect();
        match values {
            Ok(v) if v.len() == cols.len() => rows.push(Row(cols.iter().cloned().zip(v).collect())),
            _ => return Err(format!("malformed line {}", n + 2)),
        }
    }
    Ok(rows)
}

#[cfg(test)]
pub(crate) fn read(path: &std::path::Path) -> Vec<Row> {
    let text = std::fs::read_to_string(path).unwrap();
    let mut lines = text.lines();
    let cols: Vec<String> = lines.next().unwrap().split('\t').map(str::to_owned).collect();
    lines.map(|l| Row(cols.iter().cloned().zip(l.split('\t').map(|v| v.parse::<f64>().unwrap())).collect())).collect()
}

/// What `skate_events::observe` / `audio_state` would publish for this row.
#[derive(Default)]
pub(crate) struct Script {
    pub(crate) x: f32,
    air_time: f32,
    pushes: u32,
    family: Option<i32>,
    material: Option<u32>,
    jump_velocity: f32,
    /// Last row's push plant (the edge `+335`).
    planted: bool,
    /// The bridge's `+212` (|COM v|) of the last row and the last row's COM position (the
    /// fallback below), for this row's `+216`.
    com_212: f32,
    com_at: Option<[f32; 3]>,
    /// The last row's wheel positions (`Riding::wheels_before` of this row).
    wheels: Option<[[f32; 3]; 4]>,
}

impl Script {
    /// The audio state's `+216` for this row (last row's `+212`), then this row's `+212`: the
    /// logged |COM v| (`com_speed`, logs since 2026-10-03 afternoon); for older logs |Δ COM
    /// position| × 60 from the logged COM positions (`com_x/y/z`, the reckoning's followed point at
    /// 4 decimals, so ±0.006 m/s; the graph saturates from 0.95 m/s); without either 0 (graph 1.0:
    /// the impacts as logged).
    fn com_216(&mut self, r: &Row) -> f32 {
        let previous = self.com_212;
        self.com_212 = if r.0.contains_key("com_speed") {
            r.f("com_speed")
        } else if r.0.contains_key("com_x") {
            let at = [r.f("com_x"), r.f("com_y"), r.f("com_z")];
            let v = self.com_at.map_or(0.0, |p| {
                let d = [at[0] - p[0], at[1] - p[1], at[2] - p[2]];
                (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt() * 60.0
            });
            self.com_at = Some(at);
            v
        } else {
            0.0
        };
        previous
    }

    pub(crate) fn riding(&mut self, r: &Row) -> Riding {
        let speed = r.f("speed");
        let com_speed_216 = self.com_216(r);
        self.x += speed / 60.0;
        let state = r.i("state") as u32;
        // As `skate_events::observe`: an unridden board reports no contacts (logs recorded before
        // that gate still carry them).
        let unridden = super::skate_events::board_unridden(state);
        let mask = if unridden { 0 } else { r.i("wheels") as u32 };
        let contact: [bool; 4] = std::array::from_fn(|i| mask & (1 << i) != 0);
        let wheels = contact.iter().filter(|c| **c).count() as u32;
        // E2E_CONTACT_MATERIALS=1: the materials gated by contact, as the game did before 20:15.
        let lines = if std::env::var("E2E_CONTACT_MATERIALS").is_ok_and(|v| v == "1") {
            mask
        } else if unridden {
            0
        } else if r.0.contains_key("lines") {
            r.i("lines") as u32
        } else if mask != 0 {
            15
        } else {
            0
        };
        let tag = if unridden { 0 } else { r.i("tag") as u32 };
        let airborne = (200..300).contains(&state) && wheels == 0;
        // `+236` as `skate_events::audio_state`.
        let air_words = !std::env::var("E2E_AIR_WORDS").is_ok_and(|v| v == "0");
        self.air_time = super::skate_events::air_time_236(self.air_time, state, 1.0 / 60.0);
        let grinding = r.i("grinding") != 0;
        if grinding {
            self.family = Some(r.i("family") as i32);
            // The log's grind tag → `+692` (tag − 1, 0 → 143), as `skate_events::audio_state`.
            self.material = Some(material_of_tag(r.i("grind_tag") as u32));
        }
        let push = r.i("push") != 0;
        // The push plant (`skate_events::push_plant`, session review #1): logs since 2026-10-03 carry
        // State55 (`plant`), and the plant and its rise drive `+333 || +334` / `+335` and the bed's
        // push count. Older logs (no `plant` column) use the animation's push edge (`push`) unless
        // E2E_PLANT_FROM_FEET=1, which takes the plant from their foot-down bits on the ground states
        // without the brake (State55 && (State57 || State56), the bridge's `+333 || +334`; a proof
        // aid, not the game's input).
        let new_log = r.0.contains_key("plant");
        let planted = if new_log {
            Some(r.i("plant") != 0)
        } else if std::env::var("E2E_PLANT_FROM_FEET").is_ok_and(|v| v == "1") && r.0.contains_key("foot_down") {
            Some(r.i("foot_down") & 3 != 0 && r.i("brake") == 0 && (100..200).contains(&state))
        } else {
            None
        };
        let (push_planted, push_trigger) = match planted {
            Some(now) => (now, now && !self.planted),
            None => (push, push),
        };
        self.planted = planted.unwrap_or(false);
        self.pushes += u32::from(push_trigger);
        let scorable = r.i("scorable");
        let jv = r.f("jv");
        if jv != 0.0 {
            self.jump_velocity = skate_audio::player::state::jump_velocity(jv);
        }
        let feet = r.i("feet") as u32;
        let h = if new_log { Self::harness(r) } else { AudioState::default() };
        let audio = AudioState {
            dt: 1.0 / 60.0,
            ground_speed: speed,
            com_velocity: [speed, 0.0, 0.0],
            com_position: [self.x, 1.0, 0.0],
            board_position: [self.x, 0.1, 0.0],
            board_velocity: [speed, 0.0, 0.0],
            wheel_count: wheels,
            wheel_contact: contact,
            // The materials come from the wheel lines (`skate_events::audio_state`): logs since 20:15
            // carry their hit mask (`lines`); older logs and the scripts take every line as hitting
            // while any wheel is down (retail keeps wheel 0's material on 100 % of 3-wheel frames).
            wheel_material: std::array::from_fn(|i| if lines & (1 << i) != 0 { material_of_tag(tag) } else { 143 }),
            // Wheel 0's pattern for the front pair, wheel 3's for the rear (logs since the seams
            // port); scripted scenarios have none.
            seam_pattern: if unridden { [0; 4] } else {
                let (f, b) = (r.i("seam0") as u32, r.i("seam3") as u32);
                std::array::from_fn(|i| if lines & (1 << i) == 0 { 0 } else if i < 2 { f } else { b })
            },
            // The wheels around wheel 0's logged position along the deck heading (track 0.2 m,
            // wheelbase 0.6 m — the engine's board; only the grid crossings read them).
            wheel_position: {
                let (x0, z0, h) = (r.f("wheel_x"), r.f("wheel_z"), r.f("heading"));
                let (ax, az, rx, rz) = (h.sin(), h.cos(), h.cos(), -h.sin());
                let offs = [(0.0, 0.0), (-0.2, 0.0), (0.0, -0.6), (-0.2, -0.6)];
                std::array::from_fn(|i| [x0 + rx * offs[i].0 + ax * offs[i].1, 0.0, z0 + rz * offs[i].0 + az * offs[i].1])
            },
            turn: r.f("turn"),
            slope: r.f("slope"),
            airborne,
            air_time: self.air_time,
            brake: r.i("brake") != 0,
            manual_brake: r.i("manual") != 0,
            balance: r.i("balance") != 0,
            grinding,
            trick_active: scorable != -1,
            hippy_jump: airborne && scorable == 234,
            bail: h.bail,
            bail_end: h.bail_end,
            // The bail, on-foot / off-board flags, the deck spin and the body regions come from the
            // logs since 2026-10-03 (session review #10); older logs: off, as before.
            on_foot: new_log && state == 500,
            soft_wheels: h.soft_wheels,
            push_planted,
            push_trigger,
            feet_in_deck_box: [feet & 1 != 0, feet & 2 != 0],
            grind_family: self.family.unwrap_or(-1),
            // `+692` = Grinds+216 − 1 (the packer `sub_827A1B78`), as the game publishes it.
            grind_material: self.material.unwrap_or(143),
            local: true,
            jump_velocity: self.jump_velocity,
            audio_trick: -1,
            scorable: scorable as i32,
            // The scenario's slip is the sine of the deck's turn off the roll (lateral / speed).
            slip: if wheels > 0 { skate_audio::player::state::slip(r.f("slip") * speed) } else { 0.0 },
            revert: if new_log { r.i("revert") != 0 } else { state == 102 },
            offboard_308: h.offboard_308,
            deck_tilt: r.f("tilt"),
            deck_spin: h.deck_spin,
            // `+240` / `+260` (Class_Treatment w8 / w9) from the rows' to_land / jump_height (both
            // scenario kinds carry them); E2E_AIR_WORDS=0: 0 as before 2026-10-02 (no Treatments).
            air_until_landing: if air_words { r.f("to_land") } else { 0.0 },
            jump_height: if air_words { r.f("jump_height") } else { 0.0 },
            // Real-play logs carry these (state_log.rs, appended columns); scripted scenarios
            // leave them 0.
            grind_impact: r.f("grind_impact"),
            deck_impact: if unridden { 0.0 } else { r.f("deck_impact") },
            deck_material: if unridden { 143 } else { material_of_tag(r.i("deck_tag") as u32) },
            foot_speed_y: [r.f("foot_y0"), r.f("foot_y1")],
            foot_speed_xz: [r.f("foot_xz0"), r.f("foot_xz1")],
            // Off-board inputs (logs since 21:00; older logs: none).
            foot_down: [r.i("foot_down") & 1 != 0, r.i("foot_down") & 2 != 0],
            foot_material: if r.0.contains_key("foot_tag_a") { [r.i("foot_tag_a") as u32, r.i("foot_tag_b") as u32] } else { [143; 2] },
            hands_on_deck: [r.i("hands") & 1 != 0, r.i("hands") & 2 != 0],
            footstep_strength: r.f("strength"),
            foot_vertical_speed: [r.f("foot_vy_a"), r.f("foot_vy_b")],
            // The skeleton inputs (logs since the skeleton-inputs stage; older logs: the defaults).
            step_code: if r.0.contains_key("step") { r.i("step") as i32 } else { 1 },
            body_speed: r.f("body"),
            limb_speed: r.f("limb"),
            com_speed_216,
            ..h
        };
        // As `skate_events::observe`: the previous step's wheels (this row's on the first).
        let wheels_before = self.wheels.unwrap_or(audio.wheel_position);
        self.wheels = Some(audio.wheel_position);
        Riding {
            wheels_before,
            board: bevy::math::Vec3::new(self.x, 0.1, 0.0),
            speed,
            surface: tag,
            grinding,
            braking: r.i("brake") != 0 && speed > 0.5,
            wheels,
            pushes: self.pushes,
            audio,
            // The loose-board inputs (logs since the skeleton-inputs stage; older: upright, no contact).
            deck_up: if r.0.contains_key("deck_up") { r.f("deck_up") } else { 1.0 },
            deck_contact: r.i("deck_contact") != 0,
            deck_material: material_of_tag(r.i("deck_tag") as u32),
        }
    }
}

impl Script {
    /// The state-log columns appended 2026-10-03 (session review #10).
    fn harness(r: &Row) -> AudioState {
        AudioState {
            push_stroke: r.i("stroke") != 0,
            deck_spin: r.f("deck_spin"),
            deck_spin_xy: [r.f("spin_x"), r.f("spin_y")],
            bail: r.i("bail") != 0,
            bail_end: r.i("bail_end") != 0,
            offboard_308: r.i("held") != 0,
            offboard_air: r.i("offboard_air") != 0,
            footplant: r.i("footplant") != 0,
            soft_wheels: r.i("soft") != 0,
            body_slide_flag: r.i("face") != 0,
            body_impact: std::array::from_fn(|i| r.f(&format!("rimp{i}"))),
            body_slide: std::array::from_fn(|i| r.f(&format!("rslide{i}"))),
            body_tag: std::array::from_fn(|i| r.i(&format!("rtag{i}")) as u32),
            ..AudioState::default()
        }
    }
}


/// The ghost NPC skater (spec `world-audio-hookin` §3.10; user choice 2026-10-03: a replay of the
/// user's own recorded riding): the audio states of a state log's window `[from, from + seconds)`
/// as the e2e replay publishes them (every row before the window is replayed too, so the latches
/// and rings are as they were), placed in the world: the logged board / COM / wheel positions
/// moved so the window's first board position lands on `anchor`, the velocities from the
/// positions' change per row (the log keeps positions, not velocities). Requires every row to
/// parse ([`try_read`]).
pub(crate) fn ghost_states(text: &str, from: f32, seconds: f32, anchor: [f32; 3]) -> Result<Vec<AudioState>, String> {
    let rows = try_read(text)?;
    let first = (from.max(0.0) * 60.0) as usize;
    let count = (seconds.max(1.0) * 60.0) as usize;
    if first >= rows.len() {
        return Err(format!("the log has {:.1} s, the window starts at {from} s", rows.len() as f32 / 60.0));
    }
    let end = (first + count).min(rows.len());
    let mut script = Script::default();
    let origin = [rows[first].f("board_x"), rows[first].f("board_y"), rows[first].f("board_z")];
    let place = |p: [f32; 3]| -> [f32; 3] { std::array::from_fn(|i| p[i] - origin[i] + anchor[i]) };
    let mut out = Vec::with_capacity(end - first);
    let mut last: Option<([f32; 3], [f32; 3])> = None;
    for (i, r) in rows[..end].iter().enumerate() {
        let mut s = script.riding(r).audio;
        if i < first {
            continue;
        }
        let board = place([r.f("board_x"), r.f("board_y"), r.f("board_z")]);
        let com = place([r.f("com_x"), r.f("com_y"), r.f("com_z")]);
        let rate = |now: [f32; 3], then: Option<[f32; 3]>| then.map_or([0.0; 3], |t| std::array::from_fn(|k| (now[k] - t[k]) * 60.0));
        s.board_position = board;
        s.board_velocity = rate(board, last.map(|l| l.0));
        s.com_position = com;
        s.com_velocity = rate(com, last.map(|l| l.1));
        // The wheels around wheel 0's logged x, z along the deck heading (as the e2e replay), at
        // the board's height.
        let (h, w) = (r.f("heading"), place([r.f("wheel_x"), 0.0, r.f("wheel_z")]));
        let (ax, az, rx, rz) = (h.sin(), h.cos(), h.cos(), -h.sin());
        let offs = [(0.0, 0.0), (-0.2, 0.0), (0.0, -0.6), (-0.2, -0.6)];
        s.wheel_position = std::array::from_fn(|k| [w[0] + rx * offs[k].0 + ax * offs[k].1, board[1], w[2] + rz * offs[k].0 + az * offs[k].1]);
        s.local = false;
        last = Some((board, com));
        out.push(s);
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn log() -> String {
        let cols = ["ms", "speed", "wheels", "tag", "state", "board_x", "board_y", "board_z", "com_x", "com_y", "com_z", "wheel_x", "wheel_z", "heading"];
        let mut t = cols.join("\t");
        for i in 0..240 {
            let x = i as f32 * 0.1;
            t.push_str(&format!("\n{}\t6\t15\t5\t100\t{x}\t0.1\t2\t{x}\t1\t2\t{x}\t2\t0", i as f32 * 16.7));
        }
        t
    }

    #[test]
    fn a_ghost_window_is_placed_at_the_anchor_with_velocities() {
        let states = ghost_states(&log(), 1.0, 2.0, [10.0, 0.0, -5.0]).unwrap();
        assert_eq!(states.len(), 120);
        assert_eq!(states[0].board_position, [10.0, 0.0, -5.0]);
        assert!((states[10].board_velocity[0] - 6.0).abs() < 1e-3);
        assert_eq!(states[5].wheel_count, 4);
        assert!(!states[0].local);
        assert!(ghost_states(&log(), 10.0, 2.0, [0.0; 3]).is_err(), "window beyond the log");
        assert!(ghost_states("speed\tboard_x\n1\tx", 0.0, 1.0, [0.0; 3]).is_err(), "malformed");
        assert!(ghost_states("a\tb\n1\t2", 0.0, 1.0, [0.0; 3]).is_err(), "not a state log");
    }
}
