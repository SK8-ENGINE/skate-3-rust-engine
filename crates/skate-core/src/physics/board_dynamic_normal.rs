//! Dynamic wheel-response normal, original TU3 82C02388, with acceleration
//! history from82C082AC..8348. This is Ground64, distinct from wheel normals.
use super::{
    board::{BODY_COUNT, BodyId},
    board_ground::BoardGroundState,
    board_motion_output::{add, dot, inverse_length_squared, scale, subtract},
    board_runtime::BoardRuntime,
};
use crate::{math::Vector3, point_graph::PointGraph};
const UP: Vector3 = Vector3::new(0., 1., 0.);
const ZERO: Vector3 = Vector3::ZERO;
#[derive(Clone, Debug)]
pub struct DynamicNormalSettings {
    pub speed_damping: f32,
    pub up_vector_damping: f32,
    pub maximum_delta: f32,
    pub speed_scale: f32,
    pub maximum_delta_vs_speed: PointGraph<8>,
}
#[derive(Clone, Debug)]
pub struct BoardDynamicNormal {
    /// Skateboard112, published to Ground64.
    pub normal: Vector3,
    /// Skateboard128. The constructor really initializes this to UP.
    pub delta: Vector3,
    /// Skateboard144 and160, retained independently by the source.
    pub acceleration: Vector3,
    pub last_contact_normal: Vector3,
    previous_velocities: [Vector3; BODY_COUNT],
    pub part_accelerations: [Vector3; BODY_COUNT],
}
impl BoardDynamicNormal {
    ///82C00ED0 initializes112/128 to UP and144/160 to zero. The full board
    ///reset82C0D680 ->82C00D68 clears all seven previous velocities.
    pub fn new() -> Self {
        Self {
            normal: UP,
            delta: UP,
            acceleration: ZERO,
            last_contact_normal: ZERO,
            previous_velocities: [ZERO; BODY_COUNT],
            part_accelerations: [ZERO; BODY_COUNT],
        }
    }
    /// Board reset clears acceleration history but preserves wrapper112/128.
    pub fn reset_body_history(&mut self) {
        self.previous_velocities.fill(ZERO);
        self.part_accelerations.fill(ZERO);
    }
    pub fn update(
        &mut self,
        board: &BoardRuntime,
        contacts: &BoardGroundState,
        gravity: Vector3,
        dt: f32,
        previous_ground_speed: f32,
        settings: &DynamicNormalSettings,
    ) {
        let frequency = 1.0 / dt;
        for part in BodyId::ORDER {
            let i = part.index();
            let velocity = board.bodies()[i].rates.linear_velocity;
            self.part_accelerations[i] =
                scale(subtract(velocity, self.previous_velocities[i]), frequency);
            self.previous_velocities[i] = velocity;
        }
        if contacts.wheel_contact_count == 0 {
            self.acceleration = ZERO;
            return;
        }
        let mut sum = ZERO;
        //The original body tests780,781,782, then normalizes. There is no
        //fourth-wheel accumulation in this function; retain that distinction.
        for i in 0..3 {
            if contacts.parts[i].in_contact {
                sum = subtract(add(sum, self.part_accelerations[i]), gravity);
            }
        }
        let target = normalize(sum);
        self.delta = scale(self.delta, settings.speed_damping);
        let error = scale(subtract(target, self.normal), settings.up_vector_damping);
        let (direction, magnitude) = normalize_length(error);
        let maximum = settings
            .maximum_delta_vs_speed
            .evaluate(previous_ground_speed.abs() / settings.speed_scale)
            * settings.maximum_delta;
        let nonnegative = if -magnitude >= -0.0 { 0.0 } else { magnitude };
        let amount = if maximum - nonnegative >= -0.0 {
            nonnegative
        } else {
            maximum
        };
        self.delta = Vector3::new(
            direction.x.mul_add(amount, self.delta.x),
            direction.y.mul_add(amount, self.delta.y),
            direction.z.mul_add(amount, self.delta.z),
        );
        self.normal = normalize(add(self.normal, self.delta));
        self.last_contact_normal = contacts.overall_normal;
    }
}
fn normalize(v: Vector3) -> Vector3 {
    normalize_length(v).0
}
fn normalize_length(v: Vector3) -> (Vector3, f32) {
    let squared = dot(v, v);
    let inverse = inverse_length_squared(squared, 2);
    let length = if squared == 0.0 {
        0.0
    } else {
        squared * inverse
    };
    //830BD350's verified initializer is float1e-6. Source compares length.
    let normal = if length > f32::from_bits(0x3586_37bd) {
        scale(v, inverse)
    } else {
        ZERO
    };
    (normal, length)
}
