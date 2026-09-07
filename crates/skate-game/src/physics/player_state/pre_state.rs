//! Update3_State82DB6050 prefix through grab-spline query synchronization.
//! Ground/Air/KnownAir vtable24 ->82D34DA8 ->82D2D860 predicts from the actual deck part
//! transform and its physical body's linear velocity, with a single VMADD.
use super::*;
use skate_core::physics::board::BodyId;
pub(super) fn advance(physics: &mut GamePhysics, skater: &mut SkaterRuntime) -> Result<(), String> {
    let player = &mut skater.player_input.player;
    player.state_count_1312 = player.state_count_1312.wrapping_add(1);
    //KnownAir ctor82D35130 installs82327210; vslot24 at82327228 is
    //the same82D34DA8. Its target-following trajectory is not this deck sample.
    if !matches!(
        skater.player_state.current(),
        PhysicalStateId::PhysicsGround
            | PhysicalStateId::PhysicsAir
            | PhysicalStateId::GrindBoardslide | PhysicalStateId::GrindFiftyFifty
            | PhysicalStateId::Nonspecific
            | PhysicalStateId::KnownAir
            | PhysicalStateId::GroundAnimation
            | PhysicalStateId::SlideGround
            | PhysicalStateId::WipeoutGround
            | PhysicalStateId::Teleporting
            | PhysicalStateId::BipedAir
            | PhysicalStateId::BipedGround
    ) {
        return Err("PreState requires the selected state's actual PredictFutureOfDeck".into());
    }
    //BipedGround's vslot24 at82327158 calls82D2D860 directly; the riding
    //wrapper82D34DA8 calls that same producer with the same state argument.
    let deck = physics.board.part_transforms()[BodyId::Deck.index()];
    let velocity = physics.board.bodies()[BodyId::Deck.index()]
        .rates
        .linear_velocity;
    let dt = skater.player_input.processed.timestep_2604;
    let prediction = [
        velocity.x.mul_add(dt, deck.translation.x),
        velocity.y.mul_add(dt, deck.translation.y),
        velocity.z.mul_add(dt, deck.translation.z),
        0.0,
    ];
    let roots = &mut skater.animated_skeleton.roots;
    roots.predicted_board_position = prediction;
    roots.supplied_prediction = Some(prediction);
    //82DB60DC clears sticky3184, not this-update3185.
    skater.foot_ik.state.contacts.support_failed = false;
    let requests = &mut skater.ground_lifecycle.trajectory;
    //82D74270 is PlayerGrabSpline::Sync. Its288-byte grab-spline records are
    //separate from the240-byte ballistic predictions. It clears flag6 first.
    //This world's triangles have no authored grab-spline descriptors, so no
    //pending line batch and no query result9840 take the actual empty branch.
    requests.flags_12836 &= !0x40;
    if requests.pending_request.is_some() || requests.result_valid_9840 {
        return Err(
            "PreState has a retained grab-spline request/result requiring82D74270 synchronization"
                .into(),
        );
    }
    Ok(())
}
