//! Skeleton BipedAir frame production82BDDD70..82BDDFC0.
use super::{
    skeleton_animation_record::{AnimationPartTransform as Frame, compose_affine},
    skeleton_root::{SkeletonRootFrames, orthonormalize},
};
pub fn prepare(
    roots: &mut SkeletonRootFrames,
    frame: Frame,
    body_position: [f32; 4],
    local_com: [f32; 4],
    mapped: &Frame,
    flags_2476: u32,
    flags_2468: &mut u32,
) -> Frame {
    let mut frame = frame;
    if flags_2476 & 4 != 0 {
        for axis in [0, 2] {
            frame[axis] = frame[axis].map(|v| -v);
        }
    }
    frame[3] = std::array::from_fn(|i| {
        body_position[i]
            - frame[2][i].mul_add(
                local_com[2],
                frame[1][i].mul_add(local_com[1], frame[0][i] * local_com[0]),
            )
    });
    roots.initialize_heading = true;
    roots.reset_initial_alignment(orthonormalize(frame));
    *flags_2468 |= 0x80000;
    compose_affine(&roots.animation_to_world, mapped)
}
