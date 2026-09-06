//! Completed pose observations; original PhysOut template constructors.
use super::types::RawMatrix;

/// Skeleton reset82DE3A28 explicitly clears these scalars and flags.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct SkeletonOutputFields {
    pub hips_right_angle_496: f32,
    pub hips_up_angle_500: f32,
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

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct AnimationOutputFields {
    pub collision_time_144: f32,
    pub profile_148: u32,
    /// PhysOutAnimation+166, tested by OkToDoTrickOnStairs82BA6930.
    pub tricks_blocked_on_stairs_166: u8,
}
impl Default for AnimationOutputFields {
    fn default() -> Self {
        // Animation82DE3F38 stores FLT_MAX at144 and zero at148.
        // Reset82DE3F38 clears byte166. Ordinary Ground Fill82D3A388
        // and common ProcessOutput82DB6EC0 do not overwrite this byte.
        Self { collision_time_144: f32::MAX, profile_148: 0, tricks_blocked_on_stairs_166: 0 }
    }
}

/// Scoring2 reset82DE4468 clears the conditioner capability mask at204.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ScoringOutputFields {
    pub capabilities_204: u32,
}
