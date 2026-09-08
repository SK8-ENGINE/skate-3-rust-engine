use super::*;

#[derive(Clone, Copy, Debug)]
pub struct Ejection {
    pub position: [f32; 3],
    pub velocity: [f32; 3],
    pub angular_velocity: [f32; 3],
    pub reason: &'static str,
}

impl Simulation {
    pub fn set_occupied(&mut self, id: u64, occupied: bool) {
        if let Some(v) = self.vehicles.get_mut(&id) {
            if v.occupied != occupied {
                v.occupied = occupied; v.inverted_time = 0.; v.ejection = None;
                self.world.colliders[v.rider].set_enabled(occupied && v.definition.rider_safety.enabled);
                self.world.bodies[v.body].wake_up(true);
            }
        }
    }
    pub fn take_ejection(&mut self, id: u64) -> Option<Ejection> {
        self.vehicles.get_mut(&id)?.ejection.take()
    }
    pub(super) fn capture_riders(&self) -> BTreeMap<u64, (Vector, Vector)> {
        self.vehicles.iter().filter(|(_, v)| v.occupied && v.definition.rider_safety.enabled)
            .map(|(&id, v)| {
                let body=&self.world.bodies[v.body];
                let point=body.position().transform_point(Vector::from_array(v.definition.seat));
                (id, (body.linvel(), body.velocity_at_point(point)))
            }).collect()
    }
    pub(super) fn check_riders(&mut self, before: &BTreeMap<u64,(Vector,Vector)>, dt: f32) {
        for (&id, &(linear, point_velocity)) in before {
            let v=self.vehicles.get_mut(&id).unwrap();
            if v.ejection.is_some() { continue; }
            let body=&self.world.bodies[v.body];
            let safety=&v.definition.rider_safety;
            let up=body.rotation()*Vector::Y;
            v.inverted_time=if up.y < safety.inverted_up_y {v.inverted_time+dt} else {0.};
            let hit=self.world.narrow_phase.contact_pairs_with(v.rider)
                .map(|p|p.total_impulse_magnitude()).fold(0_f32,f32::max);
            let contact=body.colliders().iter().any(|&c| self.world.narrow_phase.contact_pairs_with(c)
                .any(|p|p.total_impulse_magnitude()>0.));
            let reason=if hit >= safety.hit_impulse {Some("rider_impact")}
                else if contact && (body.linvel()-linear).length() >= safety.crash_delta_v {Some("crash")}
                else if v.inverted_time >= safety.inverted_seconds {Some("inverted")}
                else {None};
            if let Some(reason)=reason {
                let seat=body.position().transform_point(Vector::from_array(v.definition.seat));
                let after=body.velocity_at_point(seat);
                let velocity=if after.length_squared()>point_velocity.length_squared() {after} else {point_velocity};
                let velocity=(velocity + Vector::Y*safety.eject_up_speed).clamp_length_max(60.);
                v.ejection=Some(Ejection { position: seat.to_array(), velocity: velocity.to_array(),
                    angular_velocity: body.angvel().clamp_length_max(15.).to_array(), reason });
            }
        }
    }
}
