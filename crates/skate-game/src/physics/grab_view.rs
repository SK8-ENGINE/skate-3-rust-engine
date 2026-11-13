//! Grab visualizer (`SKATE_VISUALIZE_GRABS=1`): records in range and the one a grab would take; viewer aid only.
//! What it draws and logs: docs/hails-additions/27-dynamic-props.md#grab-visualizer
use bevy::prelude::*;
use skate_core::player::offboard::grab_scene::{Descriptor, Record};
use skate_core::player::state::PhysicalStateId;

static LOGGING: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// The visualizer is on: the grab decisions add their per-candidate lines (`SKITCH_CANDIDATE`).
pub(crate) fn logging() -> bool {
    LOGGING.load(std::sync::atomic::Ordering::Relaxed)
}

const IN_RANGE: Color = Color::srgb(1.0, 0.7, 0.1);
const READY: Color = Color::srgb(0.2, 1.0, 0.35);

/// What a grab could take right now.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct View {
    mode: &'static str,
    /// The grab input is held: the game's own decision is shown (else the preview).
    held: bool,
    ready: Option<(u32, u32)>,
    in_range: Vec<(u32, u32)>,
}

fn mode(state: PhysicalStateId) -> &'static str {
    match state {
        PhysicalStateId::BipedGround => "on_foot",
        PhysicalStateId::Skitching => "skitching",
        _ => "riding",
    }
}

/// Records shown while grab is held (b251): the live car spot while skitching, else the ready validated list.
fn held_records(
    state: PhysicalStateId,
    owner: &super::biped_ground::grab_runtime::Owner,
    live: ([u32; 72], u32, Option<std::sync::Arc<skate_core::player::offboard::grab_scene::Geometry>>),
) -> Vec<Record> {
    if state == PhysicalStateId::Skitching {
        let (words, flags_2480, geometry) = live;
        return match geometry {
            Some(g) if flags_2480 & 0x0040_0000 != 0 => vec![Record(words, g)],
            _ => Vec::new(),
        };
    }
    if owner.flags_12836 & 0x40 != 0 { owner.validated().to_vec() } else { Vec::new() }
}

fn v3(v: [f32; 4]) -> Vec3 {
    Vec3::new(v[0], v[1], v[2])
}

/// The record a grab takes now: the Ground decision's on foot, the skitch latch's on board.
fn ready(skater: &super::SkaterRuntime) -> Option<(u32, u32)> {
    if skater.player_state.current() == PhysicalStateId::BipedGround {
        let g = &skater.biped_ground.ground;
        g.flags_144_to_150[0].then_some((g.counter_152, g.counter_156))
    } else {
        let s = &skater.ground.state;
        s.flag_2729.then_some((s.word_2560, s.word_2564))
    }
}

fn describe(physics: &super::GamePhysics, Descriptor { kind, id }: Descriptor) -> String {
    let tag = super::prop_dynamics::PROP_GRAB_TAG;
    match kind {
        2 if id & tag != 0 => {
            let prop = physics.prop_dynamics().and_then(|d| d.prop_of_grab_key(id, true));
            let template = physics.prop_dynamics().zip(prop).map_or_else(String::new, |(d, p)| d.describe_template(p));
            format!("prop {} ({template}) record {}", prop.map_or_else(|| "?".to_owned(), |p| p.to_string()), id & 63)
        }
        1 => format!("car spot {id:#x}"),
        _ => format!("kind {kind} id {id:#x}"),
    }
}

fn draw(gizmos: &mut Gizmos, record: &Record, ready: bool, time: f32) {
    let [a, b] = record.endpoints().map(v3);
    if !ready {
        gizmos.line(a, b, IN_RANGE);
        gizmos.sphere(Isometry3d::from_translation(a), 0.03, IN_RANGE);
        gizmos.sphere(Isometry3d::from_translation(b), 0.03, IN_RANGE);
        return;
    }
    for dy in [-0.015, 0.0, 0.015] {
        let up = Vec3::Y * dy;
        gizmos.line(a + up, b + up, READY);
    }
    let middle = (a + b) * 0.5;
    let pulse = 0.14 + 0.04 * (time * 6.0).sin();
    let flat = Quat::from_rotation_x(std::f32::consts::FRAC_PI_2);
    gizmos.circle(Isometry3d::new(middle, flat), pulse, READY);
    gizmos.circle(Isometry3d::new(middle, flat), pulse * 0.6, READY);
    gizmos.arrow(middle + Vec3::Y * 0.7, middle + Vec3::Y * 0.2, READY);
}

/// The same query and pick as the game's decision, without the grab input (on foot 82D324B0, on board 82D39BB8 /
/// 82D39D98); the three validation lines are not run, so a record behind a wall can show until grab is held.
fn preview_of(skater: &super::SkaterRuntime, physics: &super::GamePhysics) -> Result<(Vec<Record>, Option<(u32, u32)>), String> {
    let scene = super::offboard::grab_scene::Scene::new(&physics.world, &physics.offboard_grab_scene);
    if skater.player_state.current() == PhysicalStateId::BipedGround {
        let input = super::biped_ground::services::grab_input(skater);
        let (records, ready) = super::biped_ground::grab::preview(&scene, skater.biped_ground.grab_settings, &input)?;
        return Ok((records, ready.map(|d| (d.kind, d.id))));
    }
    let Some(toolkit) = skater.player_input.toolkit.as_ref() else { return Ok((Vec::new(), None)) };
    super::ground_runtime::skitch::preview(&scene, &skater.player_input.processed, toolkit.effective[3], &skater.ground_settings.skitch)
}

/// Draws the cues and writes `GRAB_VIEW` while the opt-in is on.
pub(crate) fn visualize_grabs(
    living: Option<Res<crate::living_world::LivingWorldSettings>>,
    skater: Option<Res<super::SkaterRuntime>>,
    physics: Option<Res<super::GamePhysics>>,
    time: Res<Time>,
    mut gizmos: Gizmos,
    mut last: Local<Option<View>>,
) {
    let on = living.as_deref().is_some_and(|l| crate::living_world::opt_ins::in_effect(l).visualize_grabs);
    LOGGING.store(on, std::sync::atomic::Ordering::Relaxed);
    let (Some(skater), Some(physics), true) = (skater, physics, on) else {
        *last = None;
        return;
    };
    let held = skater.player_input.processed.flags_2476 & 0x0040_0000 != 0;
    let preview;
    let shown;
    let (records, ready) = if held {
        let p = &skater.player_input.processed;
        shown = held_records(skater.player_state.current(), &skater.offboard_grab, (p.grab_records_1888_2176[0], p.flags_2480, skater.grab_geometry[0].clone()));
        (shown.as_slice(), ready(&skater))
    } else {
        preview = match preview_of(&skater, &physics) {
            Ok(p) => p,
            Err(e) => {
                warn!("GRAB_VIEW preview failed: {e}");
                return;
            }
        };
        (preview.0.as_slice(), preview.1)
    };
    let t = time.elapsed_secs();
    for record in records {
        let d = record.descriptor();
        draw(&mut gizmos, record, ready == Some((d.kind, d.id)), t);
    }
    let view = View {
        mode: mode(skater.player_state.current()),
        held,
        ready,
        in_range: records.iter().map(|r| (r.descriptor().kind, r.descriptor().id)).collect(),
    };
    if last.as_ref() == Some(&view) {
        return;
    }
    let position = v3(skater.player_input.processed.vectors_544_560_592_608[2].map(f32::from_bits));
    let mut parts: Vec<String> = records
        .iter()
        .map(|r| {
            let [a, b] = r.endpoints().map(v3);
            let middle = (a + b) * 0.5;
            format!("{} at {:.2} m, height {:+.2} m", describe(&physics, r.descriptor()), middle.distance(position), middle.y - position.y)
        })
        .collect();
    if parts.is_empty() {
        let nearest = physics.prop_dynamics().and_then(|d| {
            let p = skate_core::math::Vector3::new(position.x, position.y, position.z);
            let (id, centre) = d.nearest_body(p, 6.0)?;
            d.grab_object(id)?;
            let c = Vec3::new(centre.x, centre.y, centre.z);
            Some(format!("nearest prop with grab splines {id} ({}) {:.2} m to its centre", d.describe_template(id), c.distance(position)))
        });
        parts.push(nearest.unwrap_or_else(|| "no prop with grab splines within 6 m".to_owned()));
    }
    let ready_text = ready.map_or_else(|| "none".to_owned(), |(kind, id)| describe(&physics, Descriptor { kind, id }));
    info!("GRAB_VIEW {} {} ready={} in_range={} [{}]", view.mode, if held { "grab held" } else { "preview" }, ready_text, records.len(), parts.join("; "));
    *last = Some(view);
}

#[cfg(test)]
mod tests {
    use super::*;
    use skate_core::player::offboard::grab_scene::Geometry;

    fn record(x: f32) -> Record {
        let g = std::sync::Arc::new(Geometry { id: 1, points: vec![[x - 0.5, 0.5, 0.0, 1.0], [x + 0.5, 0.5, 0.0, 1.0]], approach_vectors: vec![], word_60: 0 });
        let mut r = Record([0; 72], g);
        r.set_vector(64, [x - 0.5, 0.5, 0.0, 1.0]);
        r.set_vector(80, [x + 0.5, 0.5, 0.0, 1.0]);
        r
    }

    #[test]
    fn a_held_grab_shows_the_owner_list_only_while_it_is_ready() {
        let mut owner = crate::physics::biped_ground::grab_runtime::Owner::default();
        owner.set_validated_for_test(vec![record(175.0)]);
        let none = ([0; 72], 0, None);
        assert!(held_records(PhysicalStateId::PhysicsGround, &owner, none.clone()).is_empty(), "not ready: an old list is not shown");
        owner.flags_12836 |= 0x40;
        assert_eq!(held_records(PhysicalStateId::PhysicsGround, &owner, none.clone()).len(), 1);
        owner.invalidate();
        assert!(held_records(PhysicalStateId::PhysicsGround, &owner, none).is_empty(), "invalidated: hidden again");
    }

    #[test]
    fn a_skitch_cue_follows_the_live_car_spot() {
        let mut owner = crate::physics::biped_ground::grab_runtime::Owner::default();
        owner.set_validated_for_test(vec![record(0.0)]);
        owner.flags_12836 |= 0x40;
        let live = |x: f32| { let r = record(x); (r.0, 0x0040_0000u32, Some(r.1.clone())) };
        let at = |records: &[Record]| records[0].endpoints()[0][0] + 0.5;
        assert_eq!(at(&held_records(PhysicalStateId::Skitching, &owner, live(5.0))), 5.0, "the live spot, not the latch's");
        assert_eq!(at(&held_records(PhysicalStateId::Skitching, &owner, live(6.0))), 6.0, "follows the car");
        let (words, _, g) = live(6.0);
        assert!(held_records(PhysicalStateId::Skitching, &owner, (words, 0, g)).is_empty(), "no published spot: nothing");
        assert_eq!(mode(PhysicalStateId::Skitching), "skitching");
    }
}
