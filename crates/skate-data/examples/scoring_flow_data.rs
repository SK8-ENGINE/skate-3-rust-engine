//! Original action-stream validation with supplied data, without game systems.
#[path = "../../skate-game/src/apt_display.rs"]
mod apt_display;
#[path = "../../skate-game/src/apt_movie.rs"]
mod apt_movie;
#[path = "../../skate-game/src/apt_scene.rs"]
mod apt_scene;
#[path = "../../skate-game/src/apt_text.rs"]
mod apt_text;
#[path = "../../skate-game/src/apt_vm.rs"]
mod apt_vm;
#[path = "../../skate-game/src/hud_runtime.rs"]
mod hud_runtime;
#[path = "../../skate-game/src/scoring_runtime.rs"]
mod scoring_runtime;
use skate_core::{
    animation::output::attributes::AttributeName, physics::filtered_state::FilteredCategory,
};
fn frame(
    tick: u32,
    category: FilteredCategory,
    descriptor: Option<AttributeName>,
) -> scoring_runtime::Frame {
    scoring_runtime::Frame {
        tick,
        dt: 1. / 60.,
        category,
        state: 100,
        descriptor,
        grind_id: -1,
        flags: 0,
        position: [0., 0., 0.],
        velocity: [0., 0., 5.],
        forward: [0., 0., 1.],
        rider_up: [0., 1., 0.],
        rider_forward: [0., 0., 1.],
        switch: false,
        fakie: false,
        nollie: false,
        body_flip: false,
        body_flip_side: false,
        hips_position: [0., 1., 0.],
        hips_ground: None,
        hips_surface: 0,
        suspend_air: false,
        landing: Default::default(),
        teleported: false,
        reverting: false,
    }
}
fn publish_hud(
    scoring: &scoring_runtime::Runtime,
    hud: &mut hud_runtime::Runtime,
) -> Result<(), String> {
    hud.update(
        scoring.hud_input(),
        scoring.new_trick,
        scoring.modified_trick,
        scoring.close_tricks,
    )
}
fn main() -> Result<(), String> {
    let root = std::env::args_os()
        .nth(1)
        .ok_or("Expected owned assets directory")?;
    let data = skate_data::collections::Collections::load(std::path::Path::new(&root))?;
    let mut scoring = scoring_runtime::Runtime::load(&data)?;
    let movie_path = std::env::args_os()
        .nth(2)
        .ok_or("Expected owned trickdisplay.json")?;
    let json: serde_json::Value =
        serde_json::from_slice(&std::fs::read(movie_path).map_err(|e| e.to_string())?)
            .map_err(|e| e.to_string())?;
    let mut hud = hud_runtime::Runtime::load(&json, scoring.hud_input())?;
    let kickflip = scoring
        .data
        .by_id(96)
        .ok_or("Missing kickflip")?
        .encoded_name;
    scoring.advance(frame(0, FilteredCategory::Ground, None))?;
    for tick in 1..61 {
        scoring.advance(frame(tick, FilteredCategory::Air, Some(kickflip)))?;
        publish_hud(&scoring, &mut hud)?;
    }
    for tick in 61..70 {
        scoring.advance(frame(tick, FilteredCategory::Ground, None))?;
        publish_hud(&scoring, &mut hud)?;
        if scoring.close_tricks {
            return Err("Landing emitted the native bail/close event".into());
        }
    }
    if scoring.hud_input().sequence_score != 100 || scoring.hud_input().line_time <= 0. {
        return Err("Banked landing lost its displayed score/line timer".into());
    }
    if scoring.hud_input().line_time != scoring.session.line.points / scoring.data.line_drain {
        return Err("HUD timer is not in native seconds".into());
    }
    if !hud.vm.get(hud.bindings.movie.root, "_visible").truth() {
        return Err("Original APT root disappeared on landing".into());
    }
    let banked = scoring.session.holder.snapshot.last_reward;
    if banked != 100. {
        return Err(format!(
            "Stationary unswitched kickflip, no landing bonus: expected authored 100, got {banked}"
        ));
    }
    for tick in 70..80 {
        scoring.advance(frame(tick, FilteredCategory::Ground, None))?;
        publish_hud(&scoring, &mut hud)?;
    }
    if scoring.session.holder.snapshot.last_reward != banked {
        return Err("Idle frame published the sequence again".into());
    }
    for tick in 80..140 {
        scoring.advance(frame(tick, FilteredCategory::Air, Some(kickflip)))?;
        publish_hud(&scoring, &mut hud)?;
    }
    let mut cancelled = frame(140, FilteredCategory::Ground, None);
    cancelled.teleported = true;
    scoring.advance(cancelled)?;
    if scoring.session.holder.snapshot.last_reward != 0.
        || scoring.session.holder.has_pending_sequence()
    {
        return Err("Teleport retained pending trick rewards".into());
    }
    if scoring.session.combo.multiplier != 1. {
        return Err("Teleport retained multiplier".into());
    }
    // An independent supplied-data sequence checks natural line expiry.
    let mut expired = scoring_runtime::Runtime::load(&data)?;
    expired.advance(frame(0, FilteredCategory::Ground, None))?;
    for tick in 1..61 {
        expired.advance(frame(tick, FilteredCategory::Air, Some(kickflip)))?;
    }
    for tick in 61..300 {
        expired.advance(frame(tick, FilteredCategory::Ground, None))?;
    }
    if expired.hud_input().sequence_score != 0 || expired.hud_input().line_time != 0. {
        return Err("Expired line retained its HUD score".into());
    }
    // Native Air452 suspends continuous air metrics without removing the carrier.
    let mut suspended = scoring_runtime::Runtime::load(&data)?;
    suspended.advance(frame(0, FilteredCategory::Ground, None))?;
    for tick in 1..61 {
        suspended.advance(frame(tick, FilteredCategory::Air, Some(kickflip)))?;
    }
    let held_score = suspended.hud_input().sequence_score;
    let mut freeze = frame(61, FilteredCategory::Air, Some(kickflip));
    freeze.suspend_air = true;
    freeze.position = [100., 100., 100.];
    freeze.body_flip = true;
    suspended.advance(freeze)?;
    if suspended.hud_input().sequence_score != held_score {
        return Err("Air452 suspension accrued distance/height reward".into());
    }
    // 82DA8550 banks the five air metrics by bare id. They have no authored record, so a
    // lookup that demanded one dropped every one of them and an air scored its base trick and
    // nothing else. A moving, spinning air must bank far more than its authored 100.
    let mut moving = scoring_runtime::Runtime::load(&data)?;
    moving.advance(frame(0, FilteredCategory::Ground, None))?;
    let airborne = 60;
    for tick in 1..=airborne {
        let t = tick as f32 / airborne as f32;
        let turn = t * std::f32::consts::TAU;
        let mut f = frame(tick, FilteredCategory::Air, Some(kickflip));
        // 10 m along Z, a 3 m arc of height, and one full rotation.
        f.position = [0., 3. * (t * std::f32::consts::PI).sin(), 10. * t];
        // The rider turns, not the deck: retail's spin accumulator measures the skeleton
        // root, so rotating the board alone must not register as a spin.
        f.rider_forward = [turn.sin(), 0., turn.cos()];
        moving.advance(f)?;
    }
    for tick in airborne + 1..airborne + 10 {
        moving.advance(frame(tick, FilteredCategory::Ground, None))?;
    }
    let banked = moving.session.holder.snapshot.last_reward;
    if banked <= 100. {
        return Err(format!(
            "A 10 m, 3 m-high, 360-degree air banked {banked}, which is no more than the \
             authored kickflip alone: the air metric rewards are being discarded again"
        ));
    }
    println!("Moving 360 air over 10 m and 3 m of height banked {banked:.0} (authored base 100)");

    // 82DA93D8's one-shot class-3 bonus, worth 0x600 = 4.0 times the trick's own points.
    // 82DAC780 admits it only while the hips' ground contact lies between the deck and the
    // hips, with neither vector degenerate.
    let flipped = |contact: Option<[f32; 3]>| -> Result<f32, String> {
        let mut run = scoring_runtime::Runtime::load(&data)?;
        run.advance(frame(0, FilteredCategory::Ground, None))?;
        for tick in 1..61 {
            let mut f = frame(tick, FilteredCategory::Air, Some(kickflip));
            f.position = [0., 1., 0.];
            f.hips_position = [0., 2., 0.];
            f.hips_ground = contact;
            run.advance(f)?;
        }
        for tick in 61..70 {
            run.advance(frame(tick, FilteredCategory::Ground, None))?;
        }
        Ok(run.session.holder.snapshot.last_reward)
    };
    // Contact between deck (y=1) and hips (y=2): the two vectors oppose, so it pays.
    let with_bonus = flipped(Some([0., 1.5, 0.]))?;
    // Contact below both: the vectors agree, so it must not pay.
    let without = flipped(Some([0., 0.5, 0.]))?;
    let no_contact = flipped(None)?;
    if without != 100. || no_contact != 100. {
        return Err(format!(
            "The class-3 bonus paid without its geometric gate: {without} / {no_contact}"
        ));
    }
    if with_bonus != 500. {
        return Err(format!(
            "Expected the authored 100 plus four times it, got {with_bonus}"
        ));
    }
    println!("Class-3 one-shot bonus: kickflip banks {with_bonus:.0} through the gate, {without:.0} outside it");

    // The gap/context collector: four runs of horizontal distance, two of them gated on
    // being more than 3 m and 6 m above the ground under the hips. 20 m spent above 6 m
    // tops out both of those curves, at 900 and 1800.
    let gap = |clearance: f32| -> Result<f32, String> {
        let mut run = scoring_runtime::Runtime::load(&data)?;
        // The deck is held still and above the contact: that zeroes the air distance and
        // height metrics and keeps 82DAC780's flip bonus out of the measurement, so what
        // is left is the gap runs alone.
        let mut entry = frame(0, FilteredCategory::Ground, None);
        entry.position = [0., 10., 0.];
        entry.hips_position = [0., 10., 0.];
        run.advance(entry)?;
        for tick in 1..=60 {
            // The run starts on its first active frame, so travel must begin at zero for
            // the distance to come out at exactly 20 m.
            let travelled = 20. * (tick - 1) as f32 / 59.;
            let mut f = frame(tick, FilteredCategory::Air, Some(kickflip));
            f.position = [0., 10., 0.];
            f.hips_position = [0., 10., travelled];
            f.hips_ground = Some([0., 10. - clearance, travelled]);
            run.advance(f)?;
        }
        for tick in 61..70 {
            run.advance(frame(tick, FilteredCategory::Ground, None))?;
        }
        Ok(run.session.holder.snapshot.last_reward)
    };
    let over_a_gap = gap(7.)?;
    let low = gap(1.)?;
    if low != 100. {
        return Err(format!("A 1 m clearance must clear no gap, got {low}"));
    }
    if (over_a_gap - 2800.).abs() > 0.5 {
        return Err(format!(
            "20 m spent 7 m up should bank 100 + 900 + 1800, got {over_a_gap}"
        ));
    }
    println!("Gap collector: 20 m cleared 7 m up banks {over_a_gap:.0}, the same air 1 m up banks {low:.0}");
    println!(
        "Scoring data audit: authored kickflip credited once; landing display persists; timer uses seconds; line expiry clears score; teleport cancels pending rewards and multiplier; Air452 freezes continuous metrics; air metrics reach the bank"
    );
    Ok(())
}
