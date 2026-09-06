use skate_core::physics::{
    skeleton_animation_record::{AnimationPartTransform as Transform, compose_affine},
    skeleton_root::SkeletonRootFrames,
};
pub(super) fn prepare(
    roots: &mut SkeletonRootFrames,
    world: &Transform,
    mapped: &Transform,
    retained: &mut Transform,
    flags_2476: u32,
    flags_2484: u32,
    flags_2468: &mut u32,
) -> Transform {
    let mut world = *world;
    if flags_2476 & 4 != 0 {
        //830BD4A0 initializer82F825F0 splats8216DEE0=-1 across all lanes.
        for axis in [0, 2] {
            for value in &mut world[axis] {
                *value *= -1.0;
            }
        }
    }
    roots.initialize_heading = true;
    roots.reset_initial_alignment(world);
    if flags_2484 & 1 == 0 {
        *retained = *mapped;
    }
    *flags_2468 |= 0x80000;
    compose_affine(&world, retained)
}
#[cfg(test)]
#[path = "frames_tests.rs"]
mod tests;
