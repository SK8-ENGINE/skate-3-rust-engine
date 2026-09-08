//! Infrequent, allowlisted snapshots: no player names, addresses or lobby credentials.
use bevy::prelude::*;

#[derive(Default)]
pub(crate) struct Snapshot {
    next: f64,
    state: String,
    graphics: String,
    gpu_recorded: bool,
}

pub(crate) fn sample(
    time: Res<Time<Real>>,
    config: Res<crate::config::Config>,
    physics: Res<crate::physics::GamePhysics>,
    skater: Res<crate::physics::SkaterRuntime>,
    map: Res<crate::map_transition::CurrentMap>,
    transition: Res<crate::map_transition::MapTransition>,
    menu: Option<Res<crate::graphics_menu::Menu>>,
    multiplayer: Option<Res<crate::multiplayer::Multiplayer>>,
    adapter: Option<Res<bevy::render::renderer::RenderAdapterInfo>>,
    mut previous: Local<Snapshot>,
) {
    let now = time.elapsed_secs_f64();
    if now < previous.next { return; }
    previous.next = now + 1.;
    if !previous.gpu_recorded {
        if let Some(adapter) = adapter {
            eprintln!("REPORT_META gpu={:?}", &**adapter);
            previous.gpu_recorded = true;
        }
    }
    let state = format!("map_fingerprint:{:016x} generation:{} difficulty:{} physical:{:?} paused:{} map_loading:{} multiplayer_active:{} physics_failed:{}",
        config.map_fingerprint, map.generation, config.difficulty.key(), skater.player_state.current(),
        menu.as_ref().is_some_and(|m| m.open), transition.busy(), multiplayer.as_ref().is_some_and(|m| m.active()), physics.failed);
    if state != previous.state {
        eprintln!("REPORT_TRANSITION {state}");
        previous.state = state.clone();
    }
    eprintln!("REPORT_META state={state} physics_tick:{} contacts:{} network_contacts:{}", physics.ticks, physics.contact_count, physics.network_contacts);
    if let Some(menu) = menu {
        let graphics = menu.diagnostic_settings();
        if graphics != previous.graphics {
            eprintln!("REPORT_META graphics={graphics}");
            previous.graphics = graphics;
        }
    }
}
