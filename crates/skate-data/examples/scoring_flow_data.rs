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
        switch: false,
        fakie: false,
        nollie: false,
        body_flip: false,
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
    println!(
        "Scoring data audit: authored kickflip credited once; landing display persists; timer uses seconds; line expiry clears score; teleport cancels pending rewards and multiplier"
    );
    Ok(())
}
