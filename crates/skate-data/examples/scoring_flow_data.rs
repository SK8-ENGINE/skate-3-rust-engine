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
fn main() -> Result<(), String> {
    let root = std::env::args_os()
        .nth(1)
        .ok_or("Expected owned assets directory")?;
    let data = skate_data::collections::Collections::load(std::path::Path::new(&root))?;
    let mut scoring = scoring_runtime::Runtime::load(&data)?;
    let kickflip = scoring
        .data
        .by_id(96)
        .ok_or("Missing kickflip")?
        .encoded_name;
    scoring.advance(frame(0, FilteredCategory::Ground, None))?;
    for tick in 1..61 {
        scoring.advance(frame(tick, FilteredCategory::Air, Some(kickflip)))?;
    }
    for tick in 61..70 {
        scoring.advance(frame(tick, FilteredCategory::Ground, None))?;
    }
    let banked = scoring.session.holder.snapshot.last_reward;
    if banked != 100. {
        return Err(format!(
            "Stationary unswitched kickflip, no landing bonus: expected authored 100, got {banked}"
        ));
    }
    for tick in 70..80 {
        scoring.advance(frame(tick, FilteredCategory::Ground, None))?;
    }
    if scoring.session.holder.snapshot.last_reward != banked {
        return Err("Idle frame published the sequence again".into());
    }
    for tick in 80..140 {
        scoring.advance(frame(tick, FilteredCategory::Air, Some(kickflip)))?;
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
    println!(
        "Scoring data audit: authored kickflip credited once; idle publication stable; teleport cancels pending rewards and multiplier"
    );
    Ok(())
}
