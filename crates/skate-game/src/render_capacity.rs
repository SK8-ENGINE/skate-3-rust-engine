//! Render capacity policy.
//!
//! RFC 1 D7 drops MSAA entirely and removes the auto-downgrade ladder `main`
//! carried. That ladder existed because draw count was unbounded, so the
//! renderer had to guess when the scene was too heavy and quietly reduce
//! quality. With draw count now a chosen constant (RFC 1 §6) the guessing is
//! both unnecessary and harmful: a silent quality drop hides exactly the
//! regressions the performance contract is meant to catch.
//!
//! So this module enforces one policy and reports it, rather than adapting.
use bevy::prelude::*;

pub(crate) struct RenderCapacityPlugin;
impl Plugin for RenderCapacityPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Update, enforce_msaa_off);
    }
}

/// Cameras get `Msaa` from Bevy's required components with a multisampled
/// default, so this corrects them rather than inserting. Runs every frame
/// because cameras are respawned across map transitions and by the debug camera.
fn enforce_msaa_off(mut cameras: Query<(Entity, &mut Msaa)>) {
    for (entity, mut msaa) in &mut cameras {
        if *msaa != Msaa::Off {
            debug!("RENDER_CAPACITY forcing Msaa::Off on {entity} (was {msaa:?})");
            *msaa = Msaa::Off;
        }
    }
}
