//! TU3 grind direction82D87460 and contact-point conditioner82DF0640.
use skate_core::physics::grind_contact::{FiftyFiftyCandidate, Primitive};
use skate_core::riding::ground_correction_math::dot_product;
type V = [f32; 4];

#[derive(Default)]
pub(super) struct GrindCamera {
    pub direction: V,
    pub target: V,
    previous_target: V,
    offset: V,
    midpoint: V,
    kind: u32,
    active: bool,
}
impl GrindCamera {
    pub fn exit(&mut self) {
        //82DF0610/82DF07EC invalidate history without erasing the rail tangent.
        self.active = false;
    }

    pub fn update(&mut self, contact: FiftyFiftyCandidate, edge: Primitive, kind: u32, velocity: V) {
        //82D37048 supplies the normalized primitive tangent, independently of
        //the deck's facing direction (including backwards50-50s/boardslides).
        let delta: V = core::array::from_fn(|i| edge.end[i] - edge.start[i]);
        let length = dot_product(delta, delta).sqrt();
        let mut direction = if length > 0.0 { delta.map(|v| v / length) } else { self.direction };
        let along = dot_product(direction, velocity);
        if along < 0.0 {
            direction = direction.map(|v| -v);
        }
        //82D874F0: native literals820641A8=.1 and822F8E94=-.9.
        if along.abs() < 0.1 && dot_product(self.direction, direction) < -0.9 {
            direction = direction.map(|v| -v);
        }
        self.direction = direction;

        let midpoint: V = core::array::from_fn(|i| (edge.start[i] + edge.end[i]) * 0.5);
        if !self.active {
            self.offset = [0.0; 4];
            self.target = contact.centre;
            self.previous_target = contact.centre;
        } else if kind != self.kind || midpoint[..3] != self.midpoint[..3] {
            //Preserve extrapolated motion across a change of contact owner.
            self.offset = core::array::from_fn(|i| {
                self.target[i] + (self.target[i] - self.previous_target[i]) - contact.centre[i]
            });
        }
        //Constructor82DF24CC loads literal82072818, .95 per physical tick.
        //This decays a contact discontinuity, not ordinary camera movement.
        self.offset = self.offset.map(|v| v * 0.95);
        self.previous_target = self.target;
        self.target = core::array::from_fn(|i| contact.centre[i] + self.offset[i]);
        self.midpoint = midpoint;
        self.kind = kind;
        self.active = true;
    }
}
