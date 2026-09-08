//! One render-time sample for every visible part of a fixed-step vehicle.
use bevy::prelude::*;

#[derive(Clone)]
pub(super) struct Motion {
    pub body: Transform,
    pub wheels: Vec<Transform>,
}

fn blend(a: Transform, b: Transform, alpha: f32) -> Transform {
    Transform {
        translation: a.translation.lerp(b.translation, alpha),
        rotation: a.rotation.normalize().slerp(b.rotation.normalize(), alpha),
        scale: a.scale.lerp(b.scale, alpha),
    }
}

impl Motion {
    pub fn capture(sim: &skate_vehicles::Simulation, id: u64) -> Option<Self> {
        let (p, q) = sim.pose(id)?;
        let car = sim.vehicles.get(&id)?;
        Some(Self {
            body: Transform::from_translation(Vec3::from_array(p))
                .with_rotation(Quat::from_array(q)),
            wheels: car.controller.wheels().iter().map(|wheel| {
                Transform::from_xyz(0., (car.definition.suspension_length
                    - wheel.raycast_info().suspension_length) / car.definition.model_scale, 0.)
                    .with_rotation(Quat::from_rotation_y(wheel.steering)
                        * Quat::from_rotation_x(-wheel.rotation))
            }).collect(),
        })
    }

    pub fn sample(&self, current: &Self, alpha: f32) -> Self {
        let alpha = alpha.clamp(0., 1.);
        Self {
            body: blend(self.body, current.body, alpha),
            wheels: current.wheels.iter().enumerate().map(|(i, &now)| {
                self.wheels.get(i).map_or(now, |&before| blend(before, now, alpha))
            }).collect(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn subframes_move_continuously_and_keep_seat_attached() {
        let before = Motion { body: Transform::IDENTITY, wheels: vec![Transform::IDENTITY] };
        let after = Motion {
            body: Transform::from_xyz(1., 0., 0.).with_rotation(Quat::from_rotation_y(0.2)),
            wheels: vec![Transform::from_xyz(0., 0.1, 0.)],
        };
        let seat = Vec3::new(0., 0.18, -0.2);
        for index in 0..=4 {
            let alpha = index as f32 / 4.;
            let sample = before.sample(&after, alpha);
            assert!((sample.body.translation.x - alpha).abs() < 1e-6);
            assert!((sample.wheels[0].translation.y - alpha * 0.1).abs() < 1e-6);
            let rider = sample.body.transform_point(seat);
            assert!((rider.distance(sample.body.translation) - seat.length()).abs() < 1e-6);
        }
    }
}
