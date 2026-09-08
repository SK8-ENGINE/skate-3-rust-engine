//! Completed pose observations; original PhysOut template constructors.
use super::types::RawMatrix;

/// Skeleton reset82DE3A28 explicitly clears these scalars and flags.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct SkeletonOutputFields {
    pub hips_right_angle_496: f32,
    pub hips_up_angle_500: f32,
    /// Skeleton Fill82BE2268: signed physical twist, before animation mirroring.
    pub twist_504: f32,
    pub scalar_544: f32,
    pub no_support_time_548: f32,
    pub time_until_teleport_576: f32,
    pub response_strength_580: f32,
    pub extra_weight_584: f32,
    pub response_change_588: f32,
    pub flag_597: u8,
    pub over_599: u8,
    pub flag_600: u8,
    pub flag_601: u8,
    pub teleport_pending_604: u8,
    pub response_changed_605: u8,
    pub anim_to_world_11920: RawMatrix,
}

impl SkeletonOutputFields {
    /// Skeleton Fill82BE2148..2268. Read the completed physical record, the
    /// current input toolkit's Processed352 and Reckoning1152. No pose fitting
    /// or animation mirror: GrindControlFade applies that mirror once on Begin.
    pub fn publish_twist(
        &mut self,
        record: &crate::physics::skeleton_body::SkeletonPhysicalRecord,
        processed_forward_352: [f32; 4],
        reckoning_up_1152: [f32; 4],
    ) {
        use crate::physics::{board_motion_output::inverse_length_squared, native_arithmetic::dot3};
        // SkeletonState at6464, physical pose at+1552: translations of
        // parts10/6 are Skeleton8704/8448, respectively.
        let across: [f32; 4] = std::array::from_fn(|i| record.pose[10][3][i] - record.pose[6][3][i]);
        let squared = dot3(across, across);
        let inverse = inverse_length_squared(squared, 2);
        let length = if squared == 0.0 { 0.0 } else { squared * inverse };
        // Original830BD350 is a zero vector. Preserve the ordered comparison,
        // including the zero/NaN path selecting the zero direction.
        let direction = if length > 0.0 { across.map(|v| v * inverse) } else { [0.0; 4] };
        let angle = crate::player::wipeout_state::orientation::projected_angle(
            processed_forward_352, direction, reckoning_up_1152,
        );
        // Original82139A60/50; wrap via fractional turns, not angle modulo pi.
        let turns = angle * f32::from_bits(0x3E22_F983);
        let fraction = turns - turns.floor();
        self.twist_504 = (fraction - if fraction > 0.5 { 1.0 } else { 0.0 })
            * f32::from_bits(0x40C9_0FDB);
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct AnimationOutputFields {
    pub collision_time_144: f32,
    pub profile_148: u32,
    pub tricks_blocked_on_stairs_166: u8,
    /// Ground Fill82D3A4F0 -> PhysicsWantsManualExit82BA7988.
    pub manual_opposition_168: u8,
}
impl Default for AnimationOutputFields {
    fn default() -> Self {
        // Animation82DE3F38 stores FLT_MAX at144 and zero at148.
        Self { collision_time_144: f32::MAX, profile_148: 0, tricks_blocked_on_stairs_166: 0, manual_opposition_168: 0 }
    }
}

/// Scoring2 reset82DE4468 clears the conditioner capability mask at204.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ScoringOutputFields {
    pub capabilities_204: u32,
}
