//! Typed Skitch/world-grab observations. Ground inactive82D38430 executes
//!Stop82D62F20 and predictor reset82D65FB8 before pumping. A target is present
//!only when the world query owner supplied an actual object observation.
#[derive(Clone, Debug)]
pub(crate) struct WorldGrabObservation {
    pub object: u64,
    pub position: [f32; 4],
    pub relative_velocity: [f32; 4],
}
#[derive(Clone, Debug, Default)]
pub(crate) struct GroundWorldGrabState {
    pub observation: Option<WorldGrabObservation>,
    pub contact_flags: u32,
    pub contact_time: f32,
    pub contact_position: [f32; 4],
    pub contact_velocity: [f32; 4],
    pub contact_valid: bool,
    pub active: bool,
    pub secondary_active: bool,
    pub selected_count: u32,
    pub position: [f32; 4],
    pub probe_distances: [[f32; 4]; 4],
    pub probe_weights: [[f32; 4]; 2],
    pub query_points: [[f32; 4]; 2],
    pub query_normals: [[f32; 4]; 2],
    pub query_word: u32,
    pub query_scalar: f32,
    pub query_valid: [bool; 2],
    pub output_scalar: f32,
    pub output_direction: [f32; 4],
    pub output_flags: u32,
}
impl GroundWorldGrabState {
    ///The shared vector comes from native global830BD4A0's initializer. It
    ///must be supplied by that producer, never inferred from ground normal.
    pub fn clear_inactive(&mut self, reset_direction: [f32; 4]) {
        self.active = false;
        self.secondary_active = false;
        self.contact_flags &= 0x4fff_ffff;
        self.contact_time = f32::MAX;
        self.contact_position = [0.0; 4];
        self.contact_valid = false;
        self.contact_velocity = [0.0; 4];
        self.observation = None;
        self.probe_distances = [[-1.0; 4]; 4];
        self.probe_weights = [[0.0; 4]; 2];
        self.selected_count = 0;
        self.position = [0.0; 4];
        //82D65FB8 writes two literal UnitY normals and two zero points.
        self.query_points = [[0.0; 4]; 2];
        self.query_normals = [[0.0, 1.0, 0.0, 0.0]; 2];
        self.query_word = 0;
        self.query_scalar = 0.0;
        self.query_valid = [false; 2];
        self.output_scalar = 0.0;
        self.output_direction = reset_direction;
        self.output_flags &= 0x3fff_ffff;
    }
}
