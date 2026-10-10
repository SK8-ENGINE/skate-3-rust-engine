//! The riding skater's skitch query (`sub_82D39BB8` / `sub_82D39D98`, doc 26h "Skitching step 3";
//! `.local/research/npc/b33-skitch-wiring.md` section 2, main read the call site, the box flag and the limits):
//! - the ground update runs it instead of `82D749D0` (the grab owner's invalidate) while Processed `+2476`
//!   bit 22 is set (`82D37C88` branches between the two);
//! - the box: `82D2E250(frame +192, offset, extents, out, true)` ([`skitch_bounds`]);
//! - when the owner's validated results are ready (`+12836` bit 0x40) the latch walks them: endpoints oriented
//!   by the box's right axis, CanGrabSpline (`82E08DB8`), the re-grab cooldown, the time to the spline
//!   `max(0, (dot(P - pos, fwd) - reach) / max(dot(vSkater - vCar, fwd), 0.5))`, a bind (`82585DD8` + vcall
//!   +44) per candidate, and a latch on the first one under `latch_time` (state +2729 / +2560 / +2564);
//! - always: submit the mode-255 query (`82D74200`: margin 0.25, angles 60 / 30 degrees, cap 5), which reaches
//!   the cars' provider (`+4088`).
//! NOT RETAIL YET: a type-2 candidate (world object) is not latched (its box `82D2CF68` is not decoded); `P` is
//! the closest point of the spline's endpoint chord (retail's point choice is not decoded); `+2668`
//! (`skitch_spline_height`) stays 0 and `82D91298(state+28, 2)` is not called.
use super::super::biped_ground::grab_runtime::Owner;
use skate_core::player::input_phase::ProcessedPhysicsInput;
use skate_core::player::offboard::grab_scene::{closest_point, qualify, Descriptor, Query, Record};
use skate_core::player::offboard::ground_query::QueryContext;
use skate_core::player::offboard::ground_sync::{skitch_bounds, BoardLimits, Bounds};
use skate_core::riding::grounded::state::data::PhysicsGroundState;
use skate_core::riding::skitching::SkitchQuerySettings;

fn dot(a: [f32; 4], b: [f32; 4]) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}
fn sub(a: [f32; 4], b: [f32; 4]) -> [f32; 4] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2], 0.0]
}

/// The latch is opt-in until state 104's force composition is complete (`SKATE_SKITCH=1`; doc 26h "Skitching
/// step 4e"). Without it the query, the bind and the time (`+2664`) still run, but the skater never latches.
pub(crate) fn latch_enabled() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| std::env::var("SKATE_SKITCH").is_ok_and(|v| v == "1"))
}

/// What `82D39D98` decided this frame.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(crate) struct LatchDecision {
    /// Every candidate that passed the gates was bound in order; the last bind stays.
    pub bound: Option<Descriptor>,
    /// The last computed time to the spline (`+2664`).
    pub time: Option<f32>,
    /// The first candidate under the latch time: (type, id).
    pub latch: Option<(u32, u32)>,
}

/// `82D39D98` over the owner's validated records (pure).
pub(crate) fn latch(records: &[Record], frame: [[f32; 4]; 4], position: [f32; 4], velocity: [f32; 4], bounds: Bounds, limits: BoardLimits, p: &ProcessedPhysicsInput, s: &SkitchQuerySettings) -> LatchDecision {
    let mut out = LatchDecision::default();
    let fwd = frame[2];
    for record in records {
        let Descriptor { kind, id } = record.descriptor();
        if kind != 1 {
            continue;
        }
        let [mut a, mut b] = record.endpoints();
        if dot(sub(b, a), bounds.frame[0]) < 0.0 {
            std::mem::swap(&mut a, &mut b);
        }
        if !qualify(record, position, bounds, limits) {
            continue;
        }
        if p.ground_timer_2848 > 0.0 && !(kind == 1 && id != p.skitch_value_2592) {
            continue;
        }
        let point = closest_point(position, [a, b]);
        let closing = dot(sub(velocity, record.vector(128)), fwd).max(s.min_closing);
        let time = ((dot(sub(point, frame[3]), fwd) - s.reach) / closing).max(0.0);
        out.time = Some(time);
        out.bound = Some(Descriptor { kind, id });
        if time < s.latch_time {
            out.latch = Some((kind, id));
            break;
        }
    }
    out
}

/// The query box and limits (`82D39BB8`).
pub(crate) fn query_shape(frame: [[f32; 4]; 4], s: &SkitchQuerySettings) -> (Bounds, BoardLimits) {
    let [ox, oy, oz] = s.box_offset;
    let [ex, ey, ez] = s.box_extents;
    let degrees = f32::from_bits(0x3c8e_fa35);
    (skitch_bounds(frame, [ox, oy, oz, 0.0], [ex, ey, ez, 0.0]), BoardLimits { margin: s.margin, angle_a: s.angle_a_degrees * degrees, angle_b: s.angle_b_degrees * degrees })
}

/// `82D39BB8` (and the latch `82D39D98`).
pub(crate) fn query(state: &mut PhysicsGroundState, owner: &mut Owner, p: &ProcessedPhysicsInput, s: &SkitchQuerySettings) {
    let frame = p.effective_anim_transform_192.map(|v| v.map(f32::from_bits));
    let position = p.vectors_544_560_592_608[2].map(f32::from_bits);
    let velocity = p.vectors_400_416[0].map(f32::from_bits);
    let (bounds, limits) = query_shape(frame, s);
    if owner.flags_12836 & 0x40 != 0 {
        // 82D39D98 resets its outputs each call.
        state.flag_2729 = false;
        state.word_2560 = 0;
        state.word_2564 = 0;
        state.scalar_2668 = 0.0;
        let decision = latch(owner.validated(), frame, position, velocity, bounds, limits, p, s);
        if let Some(time) = decision.time {
            state.scalar_2664 = time;
        }
        if let Some(d) = decision.bound {
            owner.request_primary(d);
        }
        if let (Some((kind, id)), true) = (decision.latch, latch_enabled()) {
            state.flag_2729 = true;
            state.word_2560 = kind;
            state.word_2564 = id;
        }
    }
    owner.query(Query {
        position,
        sort_position: position,
        bounds,
        limits,
        mode: 255,
        capacity: s.capacity,
        context: QueryContext { selection_flags_2948: p.actor_query_2948, matching_id_2952: p.actor_query_2952 as i32 },
    });
}

#[cfg(test)]
#[path = "skitch_tests.rs"]
mod tests;
