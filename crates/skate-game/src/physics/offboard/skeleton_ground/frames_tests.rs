use super::*;
use skate_core::physics::skeleton_animation_record::IDENTITY;
#[test]
fn frozen_board_uses_retained_pose_and_flips_complete_axes() {
    let mut roots = SkeletonRootFrames::default();
    let mut retained = IDENTITY;
    retained[3] = [2., 3., 4., 0.];
    let mut mapped = IDENTITY;
    mapped[3] = [9.; 4];
    let mut world = IDENTITY;
    world[0][3] = 7.;
    world[2][3] = 8.;
    let mut flags = 2;
    let output = prepare(&mut roots, &world, &mapped, &mut retained, 4, 1, &mut flags);
    assert_eq!(retained[3], [2., 3., 4., 0.]);
    assert_eq!(roots.animation_to_world[0][3], -7.);
    assert_eq!(roots.animation_to_world[2][3], -8.);
    assert_eq!(output[3][..3], [-2., 3., -4.]);
    assert_eq!(flags, 0x80002);
    assert!(roots.initialize_heading);
}
