//! Original physical pose errors82BEBEA8 and normal collision response82BE2438.
use super::{
    ExtraTargetPositions, SkeletonCollisionFeedback, SkeletonPhysicalRecord, collision_feedback::V,
    collision_vector::*,
};
use crate::physics::skeleton_animation_record::AnimationPartTransform;

#[derive(Clone, Debug, Default)]
pub struct SkeletonPoseErrors {
    /// SkeletonState4752, one vector per animated part.
    pub parts: [V; 24],
    /// SkeletonState5136/5152: the two additional physical target errors.
    pub extra: [V; 2],
    /// SkeletonState5168/5184/5200, written by UpdateExtraTargets.
    pub targets: [V; 3],
}
#[derive(Clone, Copy, Debug)]
pub struct SkeletonNormalError {
    /// Skeleton16272: physical error constrained by actual contact planes.
    pub impulse: V,
    /// Normal82BE2438 clears Skeleton16288 and16304.
    pub extra: [V; 2],
    /// Skeleton16384, length before contact-plane filtering.
    pub maximum_error: f32,
}
impl SkeletonPoseErrors {
    ///82BEBBB0 clears only target histories. The next82BEBEA8 replaces errors.
    pub fn reset_history(&mut self) {
        self.targets = [[0.0; 4]; 3];
    }
    pub fn set_targets(&mut self, positions: ExtraTargetPositions) {
        self.targets = [positions.com, positions.lifted_com, positions.following_com];
    }
    ///82BEBEA8. These are actual part-frame errors, not COM or force proxies.
    pub fn update(
        &mut self,
        physical: &SkeletonPhysicalRecord,
        animation_to_world: AnimationPartTransform,
        drive_frames: &[AnimationPartTransform; 24],
    ) {
        self.parts[0] = [0.0; 4];
        for part in 1..24 {
            // Geometric displacement vectors follow the host's W=0 convention:
            // animation frames carry native permute scratch in W, and letting
            // it into these errors would accumulate garbage into the ground
            // frame until it overflows to NaN.
            let mut error = sub(
                physical.pose[part][3],
                transform(animation_to_world, drive_frames[part][3]),
            );
            error[3] = 0.0;
            self.parts[part] = error;
        }
        for i in 0..2 {
            let mut error = sub(physical.pose[24 + i][3], self.targets[1 + i]);
            error[3] = 0.0;
            self.extra[i] = error;
        }
    }
    ///82BE2438. Strict comparison and the first raw-pair tie behavior differ
    /// from the filtered-pair tie; preserve both selection sequences.
    pub fn normal_response(
        &self,
        collision: &SkeletonCollisionFeedback,
        support_normal: V,
    ) -> SkeletonNormalError {
        let candidates = [23, 17, 18, 16, 21, 22, 20, 1].map(|part| self.parts[part]);
        let filtered =
            candidates.map(|candidate| collision.filter_error(candidate, support_normal));
        let mut impulse = filtered[0];
        for candidate in &filtered[1..] {
            if dot(*candidate, *candidate) > dot(impulse, impulse) {
                impulse = *candidate;
            }
        }
        SkeletonNormalError {
            impulse,
            extra: [[0.0; 4]; 2],
            maximum_error: self.maximum_error(),
        }
    }
    ///82BD9FB0 partial-ragdoll branch copies the two extra errors directly,
    /// then82BE2310 computes the same raw eight-part maximum length.
    pub fn partial_response(&self) -> SkeletonNormalError {
        SkeletonNormalError {
            impulse: self.extra[0],
            extra: self.extra,
            maximum_error: self.maximum_error(),
        }
    }
    fn maximum_error(&self) -> f32 {
        let candidates = [23, 17, 18, 16, 21, 22, 20, 1].map(|part| self.parts[part]);
        let mut maximum = if dot(candidates[0], candidates[0]) > dot(candidates[1], candidates[1]) {
            candidates[0]
        } else {
            candidates[1]
        };
        for candidate in &candidates[2..] {
            if dot(*candidate, *candidate) > dot(maximum, maximum) {
                maximum = *candidate;
            }
        }
        length(maximum)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::physics::skeleton_animation_record::IDENTITY;

    /// Regression for the "Nonfinite BipedAir launch packet" crash: animation
    /// frames carry native permute scratch in W, and that garbage must not
    /// leak into these geometric error vectors (it accumulated into the
    /// ground frame via ContactCorrection until it overflowed to NaN).
    #[test]
    fn garbage_w_lanes_in_frames_and_targets_do_not_leak_into_errors() {
        let mut physical = SkeletonPhysicalRecord::default();
        physical.pose[24][3] = [1.0, 2.0, 3.0, 0.0];
        physical.pose[25][3] = [4.0, 5.0, 6.0, 0.0];
        let mut errors = SkeletonPoseErrors::default();
        errors.set_targets(ExtraTargetPositions {
            com: [0.0; 4],
            lifted_com: [0.0, 1.0, 0.0, 7.0],
            following_com: [0.0, 0.0, 1.0, -13.0],
        });
        let mut drive_frames = [IDENTITY; 24];
        for frame in &mut drive_frames {
            frame[3] = [0.5, 0.5, 0.5, 99.0];
        }
        errors.update(&physical, IDENTITY, &drive_frames);
        for (part, error) in errors.parts.iter().enumerate() {
            assert_eq!(error[3], 0.0, "part {part} leaked W");
        }
        for (i, error) in errors.extra.iter().enumerate() {
            assert_eq!(error[3], 0.0, "extra {i} leaked W");
        }
        let response = errors.partial_response();
        assert_eq!(response.impulse[3], 0.0);
        assert_eq!(response.extra[0][3], 0.0);
        assert_eq!(response.extra[1][3], 0.0);
        assert!(response.maximum_error.is_finite());
    }
}
