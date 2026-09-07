//! TU3 GrindControlFade82BB0D40 and ControlGrindCrouch82BB10F8.
//! Animation attributes, physical observations and collection values remain
//! explicit inputs; these routines do not choose the grind or animation.

#[derive(Clone, Copy, Debug)]
pub struct FadeSettings {
    ///Collection owner312: offsets1640,1644,1648,1652.
    pub response: f32,
    pub input_scale: f32,
    pub acceleration: f32,
    pub maximum_step: f32,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Fade {
    pub just_began: bool,
    pub elapsed: f32,
    pub value: f32,
    pub target: f32,
    pub step: f32,
    pub minimum: f32,
    pub maximum: f32,
}

impl Fade {
    ///Begin82BB0A30 obtains bounds by evaluating the animation attribute at
    ///both endpoints. The caller supplies those evaluations and mirrored twist.
    pub fn begin(&mut self, minimum: f32, maximum: f32, twist: f32) -> f32 {
        *self = Self {
            just_began: true,
            minimum,
            maximum,
            value: bound(twist, minimum, maximum),
            target: bound(twist, minimum, maximum),
            ..Self::default()
        };
        self.value
    }

    ///The first Update after Begin only clears byte8; it emits no attribute.
    pub fn update(&mut self, dt: f32, intent: f32, settings: FadeSettings) -> Option<f32> {
        if self.just_began {
            self.just_began = false;
            return None;
        }
        self.elapsed += dt;
        self.target = bound(settings.input_scale.mul_add(intent, self.target), self.minimum, self.maximum);
        let desired = (1.0 - settings.response).mul_add(self.value, self.target * settings.response);
        let step = bound(desired - self.value, self.step - settings.acceleration, self.step + settings.acceleration);
        self.step = bound(step, -settings.maximum_step, settings.maximum_step);
        self.value = bound(self.value + self.step, self.minimum, self.maximum);
        Some(self.value)
    }
}

///Collection owner340: min1760,max1792,rate1816. Begin82BB1078 seeds
///previous with physical crouching output56+72; it does not seed from intent.
pub fn crouch(previous: f32, intent: f32, grind_crouch: f32, minimum: f32, maximum: f32, rate: f32, dt: f32) -> f32 {
    let desired = bound(1.0 - intent, minimum, maximum)
        .min(bound(1.0 - grind_crouch, minimum, maximum));
    let step = dt * rate;
    bound(desired, previous - step, previous + step)
}

fn bound(value: f32, minimum: f32, maximum: f32) -> f32 {
    //The native lower comparison precedes its upper comparison.
    let value = if minimum - value >= 0.0 { minimum } else { value };
    if maximum - value >= 0.0 { value } else { maximum }
}
