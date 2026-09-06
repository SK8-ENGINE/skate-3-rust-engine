//! Wall-jump launch packet;82D33448 constructs it,82BE33D0 fills it.
//! The native selector consumes this packet after body velocity publication.
use skate_core::{
    physics::skeleton_animation_record::{AnimationPartTransform, IDENTITY},
    riding::reckoning_frames::ReckoningFrames,
};

#[derive(Clone, Debug)]
pub(crate) struct GroundLaunchInfo {
    pub system: AnimationPartTransform,
    pub inverse_system: AnimationPartTransform,
    pub velocity: [f32; 4],
    pub angular_velocity: [f32; 4],
    pub skeleton_vector_16208: [f32; 4],
    pub skeleton_vector_16240: [f32; 4],
    pub board_position: [f32; 4],
    pub physical_center_of_mass: [f32; 4],
    pub vector_224: [f32; 4],
    pub vector_240: [f32; 4],
    pub cone_angle_x: f32,
    pub cone_angle_z: f32,
    pub time_step: f32,
    pub flag_269: bool,
    pub wall_jump: bool,
    pub flags_270: u16,
}
impl Default for GroundLaunchInfo {
    fn default() -> Self {
        Self {
            system: IDENTITY,
            inverse_system: IDENTITY,
            velocity: [0.; 4],
            angular_velocity: [0.; 4],
            skeleton_vector_16208: [0.; 4],
            skeleton_vector_16240: [0.; 4],
            board_position: [0.; 4],
            physical_center_of_mass: [0.; 4],
            vector_224: [0.; 4],
            vector_240: [0.; 4],
            cone_angle_x: 0.,
            cone_angle_z: 0.,
            time_step: 0.,
            flag_269: false,
            wall_jump: false,
            flags_270: 0,
        }
    }
}
pub(crate) struct GroundLaunchPhysical<'a> {
    pub reckoning: &'a ReckoningFrames,
    ///Actual Skeleton16208/16240; do not substitute animation COM differences.
    pub skeleton_vector_16208: [f32; 4],
    pub skeleton_vector_16240: [f32; 4],
    ///Processed112,592,400,704,2468 and2604.
    pub board_position: [f32; 4],
    pub physical_center_of_mass: [f32; 4],
    pub velocity: [f32; 4],
    pub angular_velocity: [f32; 4],
    pub flags_2468: u32,
    pub time_step: f32,
}
impl GroundLaunchInfo {
    pub fn fill(&mut self, p: &GroundLaunchPhysical<'_>, cone_angle_x: f32, cone_angle_z: f32) {
        self.board_position = p.board_position;
        self.physical_center_of_mass = p.physical_center_of_mass;
        self.skeleton_vector_16208 = p.skeleton_vector_16208;
        self.skeleton_vector_16240 = p.skeleton_vector_16240;
        self.system = p.reckoning.system;
        self.inverse_system = p.reckoning.inverse_system;
        self.velocity = p.velocity;
        self.angular_velocity = p.angular_velocity;
        self.time_step = p.time_step;
        self.flags_270 = if p.flags_2468 & 0x2000 != 0 { 7 } else { 1 };
        self.cone_angle_x = cone_angle_x;
        self.cone_angle_z = cone_angle_z;
    }
    ///Ground82D38CF4..D10 overrides both velocity records and byte268.
    pub fn wall_jump(&mut self, velocity: [f32; 4]) {
        self.velocity = velocity;
        self.angular_velocity = velocity;
        self.wall_jump = true;
    }
}
